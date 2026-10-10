//! kelta-ctl (L7): tiny CLI used by Claude hooks, compositor keybindings and scripts (SPEC §8).
//!
//! Depends only on std + serde + serde_json + rustix. Speaks the ctl socket wire format of
//! `kelta_proto::ctl` (one JSON line per request/response) without linking kelta-proto.
//!
//! - `hook` reads ≤ 1 MiB of stdin, relays it with `KELTA_SESSION_ID` / `KELTA_HOOK_TOKEN` to
//!   `KELTA_SOCK` and ALWAYS exits 0, silently.
//! - Other commands print the JSON result on stdout (exit 0) or the JSON error on stderr (exit 1).

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::{Map, Value, json};

const USAGE: &str = "usage: kelta-ctl <command>
  toggle                                   show/hide the Kelta window
  palette                                  open the command palette
  open <path>                              open a directory as a project
  focus-project <id>                       focus a project
  start <ticket key|url> [--project <id>]  start work on a ticket
  start --task <text> [--project <id>]     new work item without a ticket (wip/ branch + Claude)
  new --template <id> [--cwd <dir>] [--project <id>]
  emit <custom.event> [--json '<json>']    publish a custom.* bus event
  trust <repo path>                        review and trust a repository's .kelta config
  editor-open <file>[:line]                open a file in the editor
  plugin install <src>                     install a plugin (path, git URL or archive)
  hook                                     (internal) relay a Claude Code hook from stdin
  version                                  print versions";

const PROTOCOL: u32 = 1;
/// Hook stdin cap (PLUGINS §8).
const MAX_HOOK_STDIN: u64 = 1024 * 1024;
/// Response line cap.
const MAX_RESPONSE: u64 = 16 * 1024 * 1024;
const HOOK_TIMEOUT: Duration = Duration::from_secs(2);
const CMD_TIMEOUT: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hook") => {
            // Never fail Claude: every error is swallowed.
            let _ = hook();
            ExitCode::SUCCESS
        }
        Some("--version") | Some("-V") => {
            println!("kelta-ctl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("help") | Some("--help") | Some("-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Some(_) => run(&args),
    }
}

// ---- socket ------------------------------------------------------------------------------------

/// `KELTA_SOCK`, else the default runtime location (ARCHITECTURE §2.1).
fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("KELTA_SOCK").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    runtime_dir().join("ctl.sock")
}

fn runtime_dir() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    if cfg!(target_os = "linux")
        && let Some(x) = std::env::var_os("XDG_RUNTIME_DIR").filter(|p| !p.is_empty())
    {
        return PathBuf::from(x).join("kelta");
    }
    PathBuf::from(format!("/tmp/kelta-{uid}"))
}

/// Send one request line and read one response line.
fn request(req: &Value, timeout: Duration) -> Result<Value, String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| format!("kelta is not running (cannot connect to {}: {e})", path.display()))?;
    stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    let mut line = serde_json::to_vec(req).map_err(|e| e.to_string())?;
    line.push(b'\n');
    stream.write_all(&line).map_err(|e| format!("write: {e}"))?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut resp = Vec::new();
    BufReader::new(stream.take(MAX_RESPONSE))
        .read_until(b'\n', &mut resp)
        .map_err(|e| format!("read: {e}"))?;
    if resp.is_empty() {
        return Err("kelta closed the connection without answering".to_owned());
    }
    serde_json::from_slice(&resp).map_err(|e| format!("bad response: {e}"))
}

fn envelope(cmd: &str, fields: Value) -> Value {
    let mut m = Map::new();
    m.insert("v".into(), json!(PROTOCOL));
    m.insert("cmd".into(), json!(cmd));
    if let Value::Object(f) = fields {
        m.extend(f);
    }
    Value::Object(m)
}

// ---- hook --------------------------------------------------------------------------------------

fn hook() -> Result<(), String> {
    let session = std::env::var("KELTA_SESSION_ID").map_err(|_| "KELTA_SESSION_ID unset")?;
    let token = std::env::var("KELTA_HOOK_TOKEN").map_err(|_| "KELTA_HOOK_TOKEN unset")?;
    if session.is_empty() || token.is_empty() {
        return Err("empty session or token".into());
    }
    let mut input = Vec::new();
    std::io::stdin().lock().take(MAX_HOOK_STDIN + 1).read_to_end(&mut input).map_err(|e| e.to_string())?;
    if input.len() as u64 > MAX_HOOK_STDIN {
        return Err("hook payload larger than 1 MiB".into());
    }
    let payload: Value = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
    if !payload.is_object() {
        return Err("hook payload is not a JSON object".into());
    }
    let req = envelope("hook", json!({ "session": session, "token": token, "payload": payload }));
    request(&req, HOOK_TIMEOUT).map(|_| ())
}

// ---- commands ----------------------------------------------------------------------------------

fn absolute(p: &str) -> Result<PathBuf, String> {
    if p.is_empty() {
        return Err("empty path".into());
    }
    std::path::absolute(Path::new(p)).map_err(|e| format!("{p}: {e}"))
}

/// `file[:line]` → (absolute file, line). A trailing `:<digits>` is a line unless the whole
/// argument names an existing file.
fn file_and_line(arg: &str) -> Result<(PathBuf, Option<u32>), String> {
    if let Some((file, line)) = arg.rsplit_once(':')
        && !file.is_empty()
        && !Path::new(arg).exists()
        && let Ok(n) = line.parse::<u32>()
    {
        return Ok((absolute(file)?, Some(n)));
    }
    Ok((absolute(arg)?, None))
}

/// Split `rest` into positionals and `--flag value` / `--flag=value` options.
fn parse_opts(rest: &[String], known: &[&str]) -> Result<(Vec<String>, Map<String, Value>), String> {
    let mut pos = Vec::new();
    let mut opts = Map::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if let Some(flag) = a.strip_prefix("--") {
            let (name, value) = match flag.split_once('=') {
                Some((n, v)) => (n.to_owned(), v.to_owned()),
                None => {
                    (flag.to_owned(), it.next().ok_or_else(|| format!("--{flag} needs a value"))?.clone())
                }
            };
            if !known.contains(&name.as_str()) {
                return Err(format!("unknown option --{name}"));
            }
            opts.insert(name, Value::String(value));
        } else {
            pos.push(a.clone());
        }
    }
    Ok((pos, opts))
}

fn one(pos: &[String], what: &str) -> Result<String, String> {
    match pos {
        [x] => Ok(x.clone()),
        [] => Err(format!("missing {what}")),
        _ => Err(format!("expected one {what}, got {}", pos.len())),
    }
}

fn none(pos: &[String]) -> Result<(), String> {
    if pos.is_empty() { Ok(()) } else { Err(format!("unexpected argument {:?}", pos[0])) }
}

/// Show the repo config and its hash, ask for confirmation, return the hash. Kelta trusts the
/// file only if it still hashes to this (SETTINGS §4): what is trusted is what was read here.
fn review_trust(file: &Path) -> Result<String, String> {
    let text = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let hash = sha256_hex(&text);
    eprintln!("{}\n{}\nsha256 {hash}", file.display(), String::from_utf8_lossy(&text));
    eprint!("Trust this content? [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        return Err("not trusted".into());
    }
    Ok(hash)
}

/// SHA-256 (FIPS 180-4), inline: kelta-ctl links only std + serde + serde_json + rustix.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, c) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*c);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let t1 = hh
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// Build the request for a non-hook command.
fn build(args: &[String]) -> Result<Value, String> {
    let cmd = args[0].as_str();
    let rest = &args[1..];
    Ok(match cmd {
        "toggle" | "palette" => {
            none(rest)?;
            envelope(cmd, json!({}))
        }
        "version" => {
            none(rest)?;
            envelope("version", json!({}))
        }
        "open" => {
            let (pos, _) = parse_opts(rest, &[])?;
            let p = absolute(&one(&pos, "path")?)?;
            envelope("open", json!({ "path": p }))
        }
        "focus-project" => {
            let (pos, _) = parse_opts(rest, &[])?;
            envelope("focus_project", json!({ "id": one(&pos, "project id")? }))
        }
        "start" => {
            let (pos, opts) = parse_opts(rest, &["project", "task"])?;
            if let Some(task) = opts.get("task").and_then(Value::as_str) {
                none(&pos)?;
                if task.trim().is_empty() {
                    return Err("--task is empty".into());
                }
                return Ok(envelope("start_task", json!({ "task": task, "project": opts.get("project") })));
            }
            envelope(
                "start",
                json!({ "ticket": one(&pos, "ticket key or URL")?, "project": opts.get("project") }),
            )
        }
        "new" => {
            let (pos, opts) = parse_opts(rest, &["template", "cwd", "project"])?;
            none(&pos)?;
            let template = opts.get("template").cloned().ok_or("missing --template <id>")?;
            let cwd = match opts.get("cwd").and_then(Value::as_str) {
                Some(c) => json!(absolute(c)?),
                None => Value::Null,
            };
            envelope("new", json!({ "template": template, "cwd": cwd, "project": opts.get("project") }))
        }
        "emit" => {
            let (pos, opts) = parse_opts(rest, &["json"])?;
            let name = one(&pos, "event name")?;
            if !name.starts_with("custom.") || name.len() == "custom.".len() {
                return Err(format!("only custom.* events can be emitted, got {name:?}"));
            }
            let payload = match opts.get("json").and_then(Value::as_str) {
                Some(j) => serde_json::from_str::<Value>(j).map_err(|e| format!("--json: {e}"))?,
                None => json!({}),
            };
            envelope("emit", json!({ "name": name, "payload": payload }))
        }
        "trust" => {
            let (pos, _) = parse_opts(rest, &[])?;
            let repo = absolute(&one(&pos, "repo path")?)?;
            let sha256 = review_trust(&repo.join(".kelta").join("config.toml"))?;
            envelope("trust", json!({ "repo": repo, "sha256": sha256 }))
        }
        "editor-open" => {
            let (pos, _) = parse_opts(rest, &[])?;
            let (file, line) = file_and_line(&one(&pos, "file")?)?;
            envelope("editor_open", json!({ "file": file, "line": line }))
        }
        "plugin" => match rest.first().map(String::as_str) {
            Some("install") => {
                let (pos, _) = parse_opts(&rest[1..], &[])?;
                let src = one(&pos, "plugin source")?;
                let source =
                    if Path::new(&src).exists() { absolute(&src)?.display().to_string() } else { src };
                envelope("plugin_install", json!({ "source": source }))
            }
            _ => return Err("usage: kelta-ctl plugin install <src>".into()),
        },
        other => return Err(format!("unknown command {other:?}\n{USAGE}")),
    })
}

fn run(args: &[String]) -> ExitCode {
    let req = match build(args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("kelta-ctl: {e}");
            return ExitCode::from(2);
        }
    };
    let is_version = args[0] == "version";
    match request(&req, CMD_TIMEOUT) {
        Ok(resp) if resp.get("ok").and_then(Value::as_bool) == Some(true) => {
            let result = resp.get("result").cloned().unwrap_or(Value::Null);
            let out =
                if is_version { json!({ "ctl": env!("CARGO_PKG_VERSION"), "app": result }) } else { result };
            println!("{out}");
            ExitCode::SUCCESS
        }
        Ok(resp) => {
            let err = resp.get("error").cloned().unwrap_or(resp);
            eprintln!("{err}");
            ExitCode::FAILURE
        }
        Err(e) if is_version => {
            println!("{}", json!({ "ctl": env!("CARGO_PKG_VERSION"), "app": null, "error": e }));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", json!({ "code": "unavailable", "message": e }));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        // two blocks (56-byte message pads into a second block)
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn builds_requests() {
        assert_eq!(build(&s(&["toggle"])).unwrap(), json!({"v":1,"cmd":"toggle"}));
        assert_eq!(
            build(&s(&["start", "SHOP-1", "--project", "shop"])).unwrap(),
            json!({"v":1,"cmd":"start","ticket":"SHOP-1","project":"shop"})
        );
        assert_eq!(
            build(&s(&["emit", "custom.x", "--json", "{\"a\":1}"])).unwrap(),
            json!({"v":1,"cmd":"emit","name":"custom.x","payload":{"a":1}})
        );
        assert!(build(&s(&["emit", "pr.created"])).is_err());
        assert!(build(&s(&["new"])).is_err());
        assert!(build(&s(&["toggle", "x"])).is_err());
        let r = build(&s(&["editor-open", "/nonexistent/a.rs:12"])).unwrap();
        assert_eq!(r["file"], "/nonexistent/a.rs");
        assert_eq!(r["line"], 12);
        let r = build(&s(&["plugin", "install", "https://example.com/p.tar.gz"])).unwrap();
        assert_eq!(r["cmd"], "plugin_install");
        assert_eq!(build(&s(&["focus-project", "shop"])).unwrap()["cmd"], "focus_project");
    }
}
