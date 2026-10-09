//! Plugin install (PLUGINS §4): from a directory, a git URL (`<url>#<tag>`, shallow clone of the
//! tag) or a `.tar.gz`. `inspect` stages the source and returns the manifest, plain-language
//! permissions, the manifest SHA-256 and warnings; `install` re-stages, checks the hash the user
//! saw, copies into `<data>/plugins/<id>/` (atomic rename) and records the grants.

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use kelta_proto::error::KeltaError;
use kelta_proto::ext::{Permission, PermissionInfo, PluginInfo, PluginInstallPreview};
use kelta_proto::ids::PluginId;

use crate::PluginHost;
use crate::manifest::{self, ParsedManifest};
use crate::registry;
use crate::util::io_err;

/// Where a plugin comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Dir(PathBuf),
    Tarball(PathBuf),
    Git { url: String, tag: Option<String> },
}

/// Classify an install source string.
pub fn classify(source: &str) -> Result<Source, KeltaError> {
    let s = source.trim();
    if s.is_empty() {
        return Err(KeltaError::invalid("empty plugin source"));
    }
    let expanded = match s.strip_prefix("~/") {
        Some(rest) => {
            std::env::var("HOME").map(|h| PathBuf::from(h).join(rest)).unwrap_or_else(|_| PathBuf::from(s))
        }
        None => PathBuf::from(s),
    };
    if expanded.is_dir() {
        return Ok(Source::Dir(expanded));
    }
    if expanded.is_file() {
        let name = expanded.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".tar") {
            return Ok(Source::Tarball(expanded));
        }
        return Err(KeltaError::invalid(format!("`{s}` is not a directory or a .tar.gz archive")));
    }
    let is_git =
        ["https://", "http://", "ssh://", "git://", "file://", "git@"].iter().any(|p| s.starts_with(p));
    if is_git {
        let (url, tag) = match s.rsplit_once('#') {
            Some((u, t)) if !t.is_empty() => (u.to_owned(), Some(t.to_owned())),
            _ => (s.to_owned(), None),
        };
        if url.starts_with('-') || tag.as_deref().is_some_and(|t| t.starts_with('-')) {
            return Err(KeltaError::invalid("invalid git source"));
        }
        return Ok(Source::Git { url, tag });
    }
    Err(KeltaError::not_found(format!("plugin source `{s}` not found")))
}

/// A staged copy of a source; temporary directories are removed on drop.
struct Staged {
    root: PathBuf,
    temp: Option<PathBuf>,
    source: Source,
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(t) = &self.temp {
            let _ = std::fs::remove_dir_all(t);
        }
    }
}

async fn run(cmd: &mut tokio::process::Command, what: &str, limit: Duration) -> Result<String, KeltaError> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    // one-shot: install step deadline
    let out = tokio::time::timeout(limit, cmd.output())
        .await
        .map_err(|_| KeltaError::timeout(format!("{what} timed out")))?
        .map_err(|e| KeltaError::internal(format!("{what}: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(KeltaError::upstream(format!(
            "{what} failed: {}",
            crate::util::truncate(err.trim(), 400)
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn safe_archive_entry(name: &str) -> bool {
    let p = Path::new(name);
    !name.is_empty()
        && !p.is_absolute()
        && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Every symlink under `dir` must resolve inside it.
fn check_symlinks(dir: &Path, root: &Path) -> Result<(), KeltaError> {
    let rd = std::fs::read_dir(dir).map_err(|e| io_err(dir.display(), e))?;
    for entry in rd.filter_map(Result::ok) {
        let path = entry.path();
        let ft = entry.file_type().map_err(|e| io_err(path.display(), e))?;
        if ft.is_symlink() {
            let ok = path.canonicalize().map(|t| t.starts_with(root)).unwrap_or(false);
            if !ok {
                return Err(KeltaError::invalid(format!(
                    "archive contains a symlink leaving the plugin: {}",
                    path.display()
                )));
            }
        } else if ft.is_dir() {
            check_symlinks(&path, root)?;
        }
    }
    Ok(())
}

fn find_root(dir: &Path) -> Result<PathBuf, KeltaError> {
    if manifest::manifest_path(dir).is_some() {
        return Ok(dir.to_path_buf());
    }
    let subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io_err(dir.display(), e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')))
        .collect();
    match subdirs.as_slice() {
        [one] if manifest::manifest_path(one).is_some() => Ok(one.clone()),
        _ => Err(KeltaError::invalid(format!("no {} found in the source", manifest::MANIFEST_TOML))),
    }
}

async fn stage(host: &PluginHost, source: &str) -> Result<Staged, KeltaError> {
    let src = classify(source)?;
    match &src {
        Source::Dir(d) => {
            let root = d.canonicalize().map_err(|e| io_err(d.display(), e))?;
            Ok(Staged { root: find_root(&root)?, temp: None, source: src })
        }
        Source::Tarball(file) => {
            let temp = staging_dir(host)?;
            let staged = Staged { root: temp.clone(), temp: Some(temp.clone()), source: src.clone() };
            let listing = run(
                tokio::process::Command::new("tar").arg("-tf").arg(file),
                "tar -t",
                Duration::from_secs(60),
            )
            .await?;
            if let Some(bad) = listing.lines().find(|l| !safe_archive_entry(l.trim_end_matches('/'))) {
                return Err(KeltaError::invalid(format!("unsafe path in archive: `{bad}`")));
            }
            run(
                tokio::process::Command::new("tar")
                    .arg("-xf")
                    .arg(file)
                    .arg("--no-same-owner")
                    .arg("-C")
                    .arg(&temp),
                "tar -x",
                Duration::from_secs(120),
            )
            .await?;
            let canon = temp.canonicalize().map_err(|e| io_err(temp.display(), e))?;
            check_symlinks(&canon, &canon)?;
            let mut staged = staged;
            staged.root = find_root(&canon)?;
            Ok(staged)
        }
        Source::Git { url, tag } => {
            let temp = staging_dir(host)?;
            let staged = Staged { root: temp.clone(), temp: Some(temp.clone()), source: src.clone() };
            let target = temp.join("repo");
            let mut cmd = tokio::process::Command::new("git");
            cmd.env("GIT_TERMINAL_PROMPT", "0").args([
                "clone",
                "--depth",
                "1",
                "--single-branch",
                "--no-tags",
            ]);
            if std::env::var_os("GIT_SSH_COMMAND").is_none() {
                cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
            }
            if let Some(t) = tag {
                cmd.args(["--branch", t]);
            }
            cmd.arg("--").arg(url).arg(&target);
            run(&mut cmd, "git clone", Duration::from_secs(120)).await?;
            let canon = target.canonicalize().map_err(|e| io_err(target.display(), e))?;
            check_symlinks(&canon, &canon)?;
            let mut staged = staged;
            staged.root = find_root(&canon)?;
            Ok(staged)
        }
    }
}

fn staging_dir(host: &PluginHost) -> Result<PathBuf, KeltaError> {
    let dir = host.dirs().plugins_dir().join(".staging").join(uuid::Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&dir).map_err(|e| io_err(dir.display(), e))?;
    Ok(dir)
}

fn describe(p: &str) -> String {
    Permission::parse(p).map(|x| x.describe()).unwrap_or_else(|| format!("Unknown permission `{p}`"))
}

fn warnings_for(
    parsed: &ParsedManifest,
    previous: Option<&kelta_proto::ext::PluginManifest>,
    source: &Source,
) -> Vec<String> {
    let m = &parsed.manifest;
    let mut w = Vec::new();
    for p in &m.permissions {
        match Permission::parse(p) {
            Some(Permission::Exec(c)) => w.push(format!("This plugin can run `{c}` on your computer")),
            Some(Permission::Net(h)) => w.push(format!("This plugin can send network requests to {h}")),
            Some(Permission::TerminalWrite) => {
                w.push("This plugin can type into your terminal sessions".into())
            }
            Some(Permission::TicketsWrite) => {
                w.push("This plugin can change tickets in your trackers".into())
            }
            Some(Permission::PrsWrite) => {
                w.push("This plugin can approve and comment on pull requests".into())
            }
            _ => {}
        }
    }
    w.extend(parsed.problems.iter().cloned());
    if let Some(prev) = previous {
        w.push(format!("Replaces the installed version {}", prev.version));
        let added: Vec<&String> = m.permissions.iter().filter(|p| !prev.permissions.contains(p)).collect();
        if !added.is_empty() {
            w.push(format!(
                "This update asks for new permissions: {}",
                added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    if let Source::Git { tag: None, .. } = source {
        w.push("No tag given: installs the latest commit of the default branch".into());
    }
    w
}

pub(crate) async fn inspect(host: &PluginHost, source: &str) -> Result<PluginInstallPreview, KeltaError> {
    let staged = stage(host, source).await?;
    let parsed = manifest::load_dir(&staged.root)?;
    let reg = host.registry();
    let previous = reg.get(parsed.manifest.id.as_str()).and_then(|e| e.manifest().cloned());
    let warnings = warnings_for(&parsed, previous.as_ref(), &staged.source);
    Ok(PluginInstallPreview {
        permissions: parsed
            .manifest
            .permissions
            .iter()
            .map(|p| PermissionInfo { permission: p.clone(), description: describe(p) })
            .collect(),
        sha256: parsed.sha256.clone(),
        warnings,
        manifest: parsed.manifest,
    })
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let ft = entry.file_type()?;
        let to = dst.join(&name);
        if ft.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), &to)?;
        }
        // symlinks are not copied: installed plugins are self-contained.
    }
    Ok(())
}

pub(crate) async fn install(
    host: &PluginHost,
    source: &str,
    sha256: &str,
    grant: Vec<String>,
) -> Result<PluginInfo, KeltaError> {
    let staged = stage(host, source).await?;
    let parsed = manifest::load_dir(&staged.root)?;
    if !parsed.sha256.eq_ignore_ascii_case(sha256.trim()) {
        return Err(KeltaError::conflict(
            "the plugin manifest changed since it was inspected; inspect it again",
        )
        .with_detail(serde_json::json!({ "sha256": parsed.sha256 })));
    }
    let id = parsed.manifest.id.clone();
    let plugins = host.dirs().plugins_dir();
    let final_dir = plugins.join(id.as_str());
    let src_root = staged.root.clone();
    if src_root.canonicalize().ok() == final_dir.canonicalize().ok() {
        return Err(KeltaError::invalid("this plugin is already installed from that directory"));
    }
    host.deactivate(&id).await;
    let tmp = plugins.join(format!(".tmp-{id}-{}", uuid::Uuid::new_v4().simple()));
    let old = plugins.join(format!(".old-{id}-{}", uuid::Uuid::new_v4().simple()));
    let final_clone = final_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<(), KeltaError> {
        copy_tree(&src_root, &tmp).map_err(|e| io_err(tmp.display(), e))?;
        if final_clone.exists() {
            std::fs::rename(&final_clone, &old).map_err(|e| io_err(final_clone.display(), e))?;
        }
        if let Err(e) = std::fs::rename(&tmp, &final_clone) {
            let _ = std::fs::rename(&old, &final_clone);
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(io_err(final_clone.display(), e));
        }
        let _ = std::fs::remove_dir_all(&old);
        Ok(())
    })
    .await
    .map_err(|e| KeltaError::internal(e.to_string()))??;
    drop(staged);

    let declared = &parsed.manifest.permissions;
    let grant: Vec<String> = grant.into_iter().filter(|p| declared.contains(p)).collect();
    host.grant_store().revoke_all(&id).await?;
    if !grant.is_empty() {
        host.grant_store().grant(&id, &grant, &parsed.sha256).await?;
    }
    let reg = host.refresh();
    let entry = reg
        .entries
        .iter()
        .find(|e| e.id == id && !e.dev)
        .ok_or_else(|| KeltaError::internal("installed plugin not found after install"))?;
    host.info(entry).await
}

pub(crate) async fn uninstall(host: &PluginHost, id: &PluginId) -> Result<(), KeltaError> {
    let reg = host.registry();
    let installed = reg.entries.iter().find(|e| &e.id == id && !e.dev).cloned();
    let Some(entry) = installed else {
        if reg.get(id.as_str()).is_some() {
            return Err(KeltaError::invalid(format!(
                "`{id}` is loaded from plugins.dev_paths; remove it there"
            )));
        }
        return Err(KeltaError::not_found(format!("plugin `{id}` is not installed")));
    };
    host.deactivate(id).await;
    let dir = entry.dir.clone();
    let plugins = host.dirs().plugins_dir().canonicalize().unwrap_or_else(|_| host.dirs().plugins_dir());
    if !dir.starts_with(&plugins) {
        return Err(KeltaError::invalid("refusing to remove a directory outside the plugins directory"));
    }
    tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&dir).map_err(|e| io_err(dir.display(), e)))
        .await
        .map_err(|e| KeltaError::internal(e.to_string()))??;
    host.grant_store().revoke_all(id).await?;
    let pdir = host.dirs().plugins_dir();
    let mut state = registry::load_state(&pdir);
    if state.disabled.remove(id.as_str()) {
        registry::save_state(&pdir, &state)?;
    }
    host.refresh();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_sources() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(classify(tmp.path().to_str().unwrap()).unwrap(), Source::Dir(tmp.path().to_path_buf()));
        assert_eq!(
            classify("https://github.com/x/y.git#v1.2.0").unwrap(),
            Source::Git { url: "https://github.com/x/y.git".into(), tag: Some("v1.2.0".into()) }
        );
        assert_eq!(
            classify("git@github.com:x/y.git").unwrap(),
            Source::Git { url: "git@github.com:x/y.git".into(), tag: None }
        );
        assert!(classify("/definitely/not/here").is_err());
        assert!(classify("https://x/y#--upload-pack=evil").is_err());
    }

    #[test]
    fn archive_entries() {
        assert!(safe_archive_entry("plugin/kelta-plugin.toml"));
        assert!(safe_archive_entry("./a"));
        assert!(!safe_archive_entry("../a"));
        assert!(!safe_archive_entry("/etc/x"));
        assert!(!safe_archive_entry("a/../../b"));
    }
}
