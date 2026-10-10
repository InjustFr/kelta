//! Filesystem helpers: private runtime dirs (0700) and files (0600, atomic), `worktree.include`
//! copying, `ticket.md`, `lazygit-kelta.yml`.

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::tracker::TicketDetail;

fn io_err(path: &Path, e: std::io::Error) -> KeltaError {
    KeltaError::internal(format!("{}: {e}", path.display()))
}

/// Create `dir` (and parents) with mode 0700 for the leaf.
pub fn private_dir(dir: &Path) -> Result<(), KeltaError> {
    if let Some(parent) = dir.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| io_err(parent, e))?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| io_err(dir, e))
        }
        Err(e) => Err(io_err(dir, e)),
    }
}

/// Allocate a fresh `<runtime>/s/<key8>/` (0700). `seed` gives the preferred 8-hex key (e.g. the
/// Claude uuid); collisions fall back to random keys.
pub fn alloc_run_dir(dirs: &Dirs, seed: Option<&str>) -> Result<PathBuf, KeltaError> {
    let base = dirs.runtime.join("s");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&base).map_err(|e| io_err(&base, e))?;
    let mut candidates: Vec<String> = Vec::new();
    if let Some(s) = seed {
        let k: String = s.chars().filter(char::is_ascii_hexdigit).take(8).collect();
        if k.len() == 8 {
            candidates.push(k.to_ascii_lowercase());
        }
    }
    for _ in 0..16 {
        candidates.push(uuid::Uuid::new_v4().simple().to_string()[..8].to_owned());
    }
    for key in candidates {
        let dir = base.join(&key);
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_err(&dir, e)),
        }
    }
    Err(KeltaError::internal("could not allocate a session runtime directory"))
}

/// Write `content` to `path` atomically with mode 0600.
pub fn write_private(path: &Path, content: &[u8]) -> Result<(), KeltaError> {
    let dir =
        path.parent().ok_or_else(|| KeltaError::internal(format!("{} has no parent", path.display())))?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        uuid::Uuid::new_v4().simple()
    ));
    let res = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| io_err(&tmp, e))?;
        f.write_all(content).map_err(|e| io_err(&tmp, e))?;
        f.sync_all().map_err(|e| io_err(&tmp, e))?;
        std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// `worktree.include` + `.worktreeinclude` patterns (gitignore-like: a pattern without `/` matches a
/// basename at any depth; with `/` it is anchored at the repo root).
pub fn include_patterns(repo: &Path, configured: &[String]) -> Vec<String> {
    let mut pats: Vec<String> = configured.iter().filter(|p| !p.trim().is_empty()).cloned().collect();
    if let Ok(text) = std::fs::read_to_string(repo.join(".worktreeinclude")) {
        for line in text.lines() {
            let l = line.trim();
            if !l.is_empty() && !l.starts_with('#') && !l.starts_with('!') {
                pats.push(l.to_owned());
            }
        }
    }
    pats.dedup();
    pats
}

fn build_set(patterns: &[String]) -> Result<GlobSet, KeltaError> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim_end_matches('/');
        let anchored = p.trim_start_matches('/');
        let glob = if p.contains('/') { anchored.to_owned() } else { format!("**/{p}") };
        b.add(
            GlobBuilder::new(&glob)
                .literal_separator(true)
                .build()
                .map_err(|e| KeltaError::invalid(format!("bad include pattern `{p}`: {e}")))?,
        );
    }
    b.build().map_err(|e| KeltaError::invalid(format!("bad include patterns: {e}")))
}

/// Copy untracked/ignored `candidates` (relative to `repo`, dirs end with `/`) matching `patterns`
/// into `worktree`, never overwriting. Returns the copied relative paths.
pub fn copy_includes(
    repo: &Path,
    worktree: &Path,
    candidates: &[String],
    patterns: &[String],
) -> Result<Vec<String>, KeltaError> {
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let set = build_set(patterns)?;
    let mut copied = Vec::new();
    let mut todo = candidates.to_vec();
    while let Some(c) = todo.pop() {
        let rel = c.trim_end_matches('/');
        if rel.is_empty() {
            continue;
        }
        if set.is_match(rel) {
            copy_tree(&repo.join(rel), &worktree.join(rel), &mut copied, rel)?;
        } else if c.ends_with('/') {
            // git collapsed a fully untracked/ignored dir: a basename pattern may match inside it
            // shortcut: walks every unmatched ignored dir (node_modules, target), upgrade to a git pathspec listing if starts get slow
            let dir = repo.join(rel);
            // gone or unreadable since git listed it: nothing to copy from there
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries {
                let entry = entry.map_err(|e| io_err(&dir, e))?;
                let ft = entry.file_type().map_err(|e| io_err(&dir, e))?;
                if !ft.is_symlink() {
                    let slash = if ft.is_dir() { "/" } else { "" };
                    todo.push(format!("{rel}/{}{slash}", entry.file_name().to_string_lossy()));
                }
            }
        }
    }
    copied.sort();
    Ok(copied)
}

fn copy_tree(src: &Path, dst: &Path, copied: &mut Vec<String>, rel: &str) -> Result<(), KeltaError> {
    let meta = std::fs::symlink_metadata(src).map_err(|e| io_err(src, e))?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dst).map_err(|e| io_err(dst, e))?;
        for entry in std::fs::read_dir(src).map_err(|e| io_err(src, e))? {
            let entry = entry.map_err(|e| io_err(src, e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            copy_tree(&entry.path(), &dst.join(&name), copied, &format!("{rel}/{name}"))?;
        }
        return Ok(());
    }
    if dst.exists() {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    std::fs::copy(src, dst).map_err(|e| io_err(dst, e))?;
    copied.push(rel.to_owned());
    Ok(())
}

/// Very small HTML → text for comment bodies (tags dropped, common entities decoded).
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for c in html.chars() {
        match (in_tag, c) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                let t = tag.trim_start_matches('/').to_ascii_lowercase();
                if t.starts_with("br") || t.starts_with('p') || t.starts_with("li") || t.starts_with("div") {
                    out.push('\n');
                }
            }
            (true, c) => tag.push(c),
            (false, c) => out.push(c),
        }
    }
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    out.trim().to_owned()
}

/// `ticket.md`: key, title, URL, status, labels, Markdown body, last 10 comments.
pub fn ticket_markdown(d: &TicketDetail) -> String {
    let t = &d.ticket;
    let mut s = format!("# {}: {}\n\n", t.r#ref.key, t.title);
    s.push_str(&format!("- URL: {}\n- Status: {}\n", t.url, t.status.name));
    if let Some(k) = &t.kind {
        s.push_str(&format!("- Type: {k}\n"));
    }
    if !t.labels.is_empty() {
        s.push_str(&format!("- Labels: {}\n", t.labels.join(", ")));
    }
    if let Some(p) = &t.priority {
        s.push_str(&format!("- Priority: {p}\n"));
    }
    s.push_str("\n## Description\n\n");
    s.push_str(if d.body_md.trim().is_empty() { "_(empty)_" } else { d.body_md.trim() });
    s.push('\n');
    let start = d.comments.len().saturating_sub(10);
    if start < d.comments.len() {
        s.push_str("\n## Comments\n");
        for c in &d.comments[start..] {
            s.push_str(&format!(
                "\n### {} — {}\n\n{}\n",
                c.author.name,
                c.created_at,
                html_to_text(&c.body_html)
            ));
        }
    }
    s
}

/// `<data>/lazygit-kelta.yml`: lazygit opens files through `kelta-ctl editor-open`.
pub fn lazygit_config(ctl: &Path) -> String {
    let q = crate::template::shell_quote(&ctl.to_string_lossy());
    // YAML double-quoted scalars: escape backslashes and double quotes.
    let y = |s: String| s.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "# Generated by Kelta — do not edit (rewritten at startup).\nos:\n  edit: \"{}\"\n  editAtLine: \"{}\"\n  editAtLineAndWait: \"{}\"\n  openDirInEditor: \"{}\"\n",
        y(format!("{q} editor-open {{{{filename}}}}")),
        y(format!("{q} editor-open {{{{filename}}}}:{{{{line}}}}")),
        y(format!("{q} editor-open {{{{filename}}}}:{{{{line}}}}")),
        y(format!("{q} editor-open {{{{dir}}}}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_copy_and_match() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let wt = tmp.path().join("wt");
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(repo.join(".env"), "A=1").unwrap();
        std::fs::write(repo.join(".env.local"), "B=1").unwrap();
        std::fs::write(repo.join("sub/.env"), "C=1").unwrap();
        std::fs::write(repo.join("other.txt"), "x").unwrap();
        std::fs::write(repo.join(".worktreeinclude"), "# c\nother.txt\n").unwrap();
        let pats = include_patterns(&repo, &[".env".into(), ".env.*".into()]);
        assert_eq!(pats, vec![".env", ".env.*", "other.txt"]);
        let cands: Vec<String> = [".env", ".env.local", "sub/", "other.txt", "node_modules/"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let copied = copy_includes(&repo, &wt, &cands, &[".env".into(), ".env.*".into()]).unwrap();
        assert_eq!(
            copied,
            vec![".env", ".env.local", "sub/.env"],
            "basename patterns match inside collapsed dirs"
        );
        assert!(wt.join(".env").exists());
        assert!(!wt.join("other.txt").exists());
        // Never overwrites.
        std::fs::write(wt.join(".env"), "mine").unwrap();
        copy_includes(&repo, &wt, &cands, &pats).unwrap();
        assert_eq!(std::fs::read_to_string(wt.join(".env")).unwrap(), "mine");
    }

    #[test]
    fn private_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::under(tmp.path());
        let a = alloc_run_dir(&dirs, Some("6f1d2c3b-4a59")).unwrap();
        assert!(a.ends_with("s/6f1d2c3b"));
        let b = alloc_run_dir(&dirs, Some("6f1d2c3b-4a59")).unwrap();
        assert_ne!(a, b);
        let mode = std::fs::metadata(&a).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let f = a.join("x.json");
        write_private(&f, b"{}").unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn lazygit_yaml() {
        let y = lazygit_config(Path::new("/Users/a/Library/Application Support/x/kelta-ctl"));
        assert!(y.contains(
            "editAtLine: \"'/Users/a/Library/Application Support/x/kelta-ctl' editor-open {{filename}}:{{line}}\""
        ));
    }
}
