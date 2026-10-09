//! Editor adapters (SETTINGS `[editor]`): nvim RPC, vim keys, `open_cmd` commands, external editors.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use kelta_proto::error::KeltaError;
use kelta_proto::settings::{EditorPreset, EditorSettings};

use crate::template::{Ctx, Mode, render};

/// Preset by id (enabled only), falling back to `editor.default`, then `nvim`.
pub fn preset<'a>(cfg: &'a EditorSettings, id: Option<&str>) -> Option<&'a EditorPreset> {
    let find = |id: &str| cfg.presets.iter().find(|p| p.id == id && p.enabled);
    id.and_then(find).or_else(|| find(&cfg.default)).or_else(|| find("nvim"))
}

/// Translate Vim key notation (`<C-\><C-N>:edit +12 a.rs<CR>`) into terminal bytes.
pub fn vim_keys(notation: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = notation;
    while let Some(start) = rest.find('<') {
        out.extend_from_slice(&rest.as_bytes()[..start]);
        let tail = &rest[start..];
        let Some(end) = tail[1..].find('>').map(|i| i + 1) else {
            out.extend_from_slice(tail.as_bytes());
            rest = "";
            break;
        };
        // `<C->>` style: allow a `>` right after the dash.
        let (name, consumed) = if tail[1..].to_ascii_lowercase().starts_with("c->") {
            ("C->", 5)
        } else {
            (&tail[1..end], end + 1)
        };
        match key_bytes(name) {
            Some(b) => {
                out.extend_from_slice(&b);
                rest = &tail[consumed..];
            }
            None => {
                out.push(b'<');
                rest = &tail[1..];
            }
        }
    }
    out.extend_from_slice(rest.as_bytes());
    out
}

fn key_bytes(name: &str) -> Option<Vec<u8>> {
    let lower = name.to_ascii_lowercase();
    let simple: Option<&[u8]> = match lower.as_str() {
        "cr" | "enter" | "return" => Some(b"\r"),
        "esc" => Some(b"\x1b"),
        "tab" => Some(b"\t"),
        "space" => Some(b" "),
        "lt" => Some(b"<"),
        "bar" => Some(b"|"),
        "bslash" => Some(b"\\"),
        "bs" | "backspace" => Some(b"\x7f"),
        "nl" => Some(b"\n"),
        _ => None,
    };
    if let Some(s) = simple {
        return Some(s.to_vec());
    }
    let key = lower.strip_prefix("c-")?;
    let c = key.chars().next()?;
    if key.chars().count() != 1 {
        return None;
    }
    let b = match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' => 0,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' => 0x1f,
        _ => return None,
    };
    Some(vec![b])
}

/// Escape a file name for an Ex command typed as keys (`:edit a\ b.rs`). `<` becomes `<lt>` so
/// `vim_keys` never turns a name like `x<CR>:!cmd<CR>` into keystrokes.
pub fn ex_escape(path: &str) -> String {
    let mut s = String::with_capacity(path.len());
    for c in path.chars() {
        if c == '<' {
            s.push_str("<lt>");
            continue;
        }
        if " \t\\|\"%#'*?[{$`".contains(c) {
            s.push('\\');
        }
        s.push(c);
    }
    s
}

/// Placeholder context for an editor preset.
pub fn ctx(
    sock: Option<&Path>,
    path: &str,
    file: Option<&Path>,
    line: Option<u32>,
    cwd: &Path,
    sid8: &str,
) -> Ctx {
    let mut c = Ctx::new();
    c.set("sock", sock.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    c.set("path", path);
    c.set("file", file.map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
    c.set("line", line.unwrap_or(1).to_string());
    c.set("cwd", cwd.to_string_lossy().into_owned());
    c.set("sid8", sid8);
    c
}

/// Render each argv element of a preset template.
pub fn render_args(args: &[String], ctx: &Ctx) -> Result<Vec<String>, KeltaError> {
    args.iter().map(|a| render(a, ctx, Mode::Lenient)).collect()
}

/// Run an `open_cmd` / external launcher detached from Kelta's PTYs (stdin null, output dropped).
/// Waits at most 10 s for short-lived clients (`emacsclient -n`, `code -g`) so they are reaped.
pub async fn run_detached(argv: &[String], cwd: &Path) -> Result<(), KeltaError> {
    let (prog, args) = argv.split_first().ok_or_else(|| KeltaError::invalid("editor command is empty"))?;
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0);
    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            KeltaError::not_found(format!("`{prog}` not found in PATH"))
        } else {
            KeltaError::internal(format!("{prog}: {e}"))
        }
    })?;
    // one-shot: reap the launcher; a long-running GUI editor keeps running on its own.
    match tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()).await {
        Ok(Ok(out)) if !out.status.success() => Err(KeltaError::upstream(format!(
            "{prog} exited with {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
        Ok(Err(e)) => Err(KeltaError::internal(format!("{prog}: {e}"))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(vim_keys("<C-\\><C-N>:edit +12 a.rs<CR>"), b"\x1c\x0e:edit +12 a.rs\r".to_vec());
        assert_eq!(vim_keys("<Esc>i<lt>x<Unknown>"), b"\x1bi<x<Unknown>".to_vec());
        assert_eq!(ex_escape("my file#1.rs"), "my\\ file\\#1.rs");
        // A hostile file name stays one literal argument.
        let typed = vim_keys(&format!(":edit {}<CR>", ex_escape("x<CR>:!touch<Space>p")));
        assert_eq!(typed, b":edit x<CR>:!touch<Space>p\r".to_vec());
    }

    #[test]
    fn presets() {
        let cfg = EditorSettings::default();
        assert_eq!(preset(&cfg, Some("vim")).map(|p| p.id.as_str()), Some("vim"));
        assert_eq!(preset(&cfg, Some("nope")).map(|p| p.id.as_str()), Some("nvim"));
        let c = ctx(Some(Path::new("/r/s/abcd/nvim.sock")), ".", None, None, Path::new("/w"), "abcd");
        let nvim = preset(&cfg, None).unwrap();
        assert_eq!(render_args(&nvim.args, &c).unwrap(), vec!["--listen", "/r/s/abcd/nvim.sock", "."]);
    }
}
