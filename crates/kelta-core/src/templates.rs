//! `session_spawn_template` (SETTINGS `[[session_templates]]`, SPEC §3.2): spawn every leaf of a
//! template and open them as one layout subtree. Standalone Claude sessions get the generated
//! hooks file (PLUGINS §8) and a `--session-id` so they report status and can be resumed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kelta_proto::error::KeltaError;
use kelta_proto::events::{Toast, UiEvent};
use kelta_proto::ext::ToolHandle;
use kelta_proto::ids::{ProjectId, SessionId, ToolId};
use kelta_proto::model::{
    LayoutNode, OpenPaneRequest, PaneContent, Placement, RestorePolicy, SessionInfo, SessionKind,
    SpawnRequest, TemplateCtx,
};
use kelta_proto::settings::{ClaudeSettings, EditorPreset, Settings, TemplateNode};

use crate::Core;
use crate::layout;

/// Render `{name}` placeholders.
pub fn render(t: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = t.to_owned();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// The hooks Kelta registers for a Claude session (PLUGINS §8) + `claude.extra_hooks`.
pub fn claude_hooks_settings(ctl: &Path, extra: &BTreeMap<String, serde_json::Value>) -> serde_json::Value {
    use kelta_proto::hooks::names;
    let cmd = format!("'{}' hook", ctl.display().to_string().replace('\'', r"'\''"));
    let hook = |async_: bool| {
        let mut h = serde_json::json!({ "type": "command", "command": cmd, "timeout": 5 });
        if async_ {
            h["async"] = serde_json::json!(true);
        }
        h
    };
    let mut hooks = serde_json::Map::new();
    for name in names::ALL {
        let mut entry = serde_json::json!({ "hooks": [hook(*name != names::SESSION_END)] });
        if *name == names::NOTIFICATION {
            entry["matcher"] = serde_json::json!(names::NOTIFICATION_MATCHER);
        } else if *name == names::POST_TOOL_USE {
            entry["matcher"] = serde_json::json!(names::EDIT_TOOLS_MATCHER);
        }
        hooks.insert((*name).to_owned(), serde_json::json!([entry]));
    }
    for (event, v) in extra {
        let list = hooks.entry(event.clone()).or_insert_with(|| serde_json::json!([]));
        match (list.as_array_mut(), v) {
            (Some(l), serde_json::Value::Array(more)) => l.extend(more.iter().cloned()),
            (Some(l), other) => l.push(other.clone()),
            _ => {}
        }
    }
    serde_json::json!({ "hooks": hooks })
}

/// Claude argv for a standalone/template session.
pub fn claude_args(
    claude: &ClaudeSettings,
    profile: Option<&str>,
    uuid: &str,
    settings_file: &Path,
    prompt: Option<&str>,
) -> Vec<String> {
    let mut a = vec!["--session-id".to_owned(), uuid.to_owned()];
    let p = profile.and_then(|p| claude.profiles.get(p)).or_else(|| claude.profiles.get("default"));
    if let Some(p) = p {
        let effort = serde_json::to_value(p.effort).ok().and_then(|v| v.as_str().map(str::to_owned));
        let mode = serde_json::to_value(p.permission_mode).ok().and_then(|v| v.as_str().map(str::to_owned));
        if !p.model.is_empty() {
            a.extend(["--model".to_owned(), p.model.clone()]);
        }
        if let Some(e) = effort {
            a.extend(["--effort".to_owned(), e]);
        }
        if let Some(m) = mode {
            a.extend(["--permission-mode".to_owned(), m]);
        }
    }
    a.extend(["--settings".to_owned(), settings_file.to_string_lossy().into_owned()]);
    for t in &claude.allowed_tools {
        a.extend(["--allowedTools".to_owned(), t.clone()]);
    }
    a.extend(claude.extra_args.iter().cloned());
    if let Some(p) = prompt.filter(|p| !p.trim().is_empty()) {
        a.push(p.to_owned());
    }
    a
}

fn write_private(path: &Path, text: &str) -> Result<(), KeltaError> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    std::io::Write::write_all(&mut f, text.as_bytes())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn leaves(n: &TemplateNode, out: &mut Vec<TemplateNode>) {
    match n {
        TemplateNode::Split { children, .. } => children.iter().for_each(|c| leaves(c, out)),
        leaf => out.push(leaf.clone()),
    }
}

/// Template tree → layout tree, consuming one pane content per leaf (`None` = leaf skipped).
fn to_layout(n: &TemplateNode, contents: &mut std::vec::IntoIter<Option<PaneContent>>) -> Option<LayoutNode> {
    match n {
        TemplateNode::Split { split, ratios, children } => {
            let mut kids = Vec::new();
            let mut rs = Vec::new();
            for (i, c) in children.iter().enumerate() {
                if let Some(k) = to_layout(c, contents) {
                    kids.push(k);
                    rs.push(ratios.get(i).copied().unwrap_or(0.0));
                }
            }
            match kids.len() {
                0 => None,
                1 => kids.pop(),
                _ => Some(layout::normalize_tree(LayoutNode::Split {
                    dir: *split,
                    ratios: rs,
                    children: kids,
                })),
            }
        }
        TemplateNode::Session { .. } => contents.next().flatten().map(layout::pane),
    }
}

impl Core {
    fn editor_preset(&self, s: &Settings) -> Option<EditorPreset> {
        s.editor
            .presets
            .iter()
            .find(|p| p.id == s.editor.default && p.enabled)
            .or_else(|| s.editor.presets.iter().find(|p| p.enabled))
            .cloned()
    }

    /// Spawn one template leaf; returns the pane content (+ the session) or `None` when skipped.
    #[allow(clippy::too_many_arguments)]
    async fn spawn_leaf(
        &self,
        leaf: &TemplateNode,
        project: &ProjectId,
        cwd: &Path,
        ctx: &TemplateCtx,
        s: &Settings,
        template_id: &str,
        vars: &BTreeMap<&str, String>,
    ) -> Result<Option<(PaneContent, Option<SessionInfo>)>, KeltaError> {
        let TemplateNode::Session { session, name, profile, command } = leaf else { return Ok(None) };
        let base = SpawnRequest {
            id: None,
            project_id: project.clone(),
            kind: SessionKind::Shell,
            name: name.clone(),
            program: None,
            args: Vec::new(),
            cwd: Some(cwd.to_path_buf()),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
            work_item_id: ctx.work_item_id.clone(),
            restore: RestorePolicy::None,
            close_on_exit: Default::default(),
            template_id: Some(template_id.to_owned()),
        };
        let term = |i: &SessionInfo| PaneContent::Terminal { session_id: i.id.clone() };
        match session.as_str() {
            "shell" => {
                let info = self.spawn_session(base).await?;
                if let Some(c) = command.as_deref().filter(|c| !c.trim().is_empty()) {
                    let line = format!("{}\r", render(c, vars));
                    let _ = self.write_session(&info.id, line.as_bytes());
                }
                Ok(Some((term(&info), Some(info))))
            }
            "setup" => {
                if s.worktree.setup.is_empty() {
                    return Ok(None);
                }
                let info = self.spawn_session(SpawnRequest { kind: SessionKind::Setup, ..base }).await?;
                let line = format!("{}\r", s.worktree.setup.join(" && "));
                let _ = self.write_session(&info.id, line.as_bytes());
                Ok(Some((term(&info), Some(info))))
            }
            "claude" => {
                let id = self.fresh_session_id();
                let run = self.dirs.session_runtime(&id.sid8());
                let file = run.join("claude-settings.json");
                let hooks = claude_hooks_settings(&self.dirs.stable_ctl(), &s.claude.extra_hooks);
                write_private(&file, &serde_json::to_string_pretty(&hooks)?)?;
                let uuid = uuid::Uuid::new_v4().to_string();
                let prompt = s.claude.prompt_templates.get("standalone").map(|t| render(t, vars));
                let args = claude_args(&s.claude, profile.as_deref(), &uuid, &file, prompt.as_deref());
                let req = SpawnRequest {
                    id: None,
                    kind: SessionKind::Claude,
                    program: Some(s.claude.binary.clone()),
                    args,
                    restore: RestorePolicy::ClaudeResume { uuid },
                    ..base
                };
                let info = self.spawn_session_with_id(id, req).await?;
                Ok(Some((term(&info), Some(info))))
            }
            "editor" => {
                let Some(preset) = self.editor_preset(s) else {
                    let info = self.spawn_session(base).await?;
                    return Ok(Some((term(&info), Some(info))));
                };
                let id = self.fresh_session_id();
                let run = self.dirs.session_runtime(&id.sid8());
                let mut v = vars.clone();
                v.insert("sock", run.join("nvim.sock").to_string_lossy().into_owned());
                v.insert("path", ".".into());
                v.insert("sid8", id.sid8());
                let mut args: Vec<String> = preset.args.iter().map(|a| render(a, &v)).collect();
                if template_id == s.work.review_template {
                    args.extend(s.editor.review_args.iter().map(|a| render(a, &v)));
                }
                if preset.external {
                    let mut cmd = tokio::process::Command::new(&preset.command);
                    cmd.args(&args).current_dir(cwd).envs(self.login_env.vars.iter());
                    cmd.stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    if let Ok(mut child) = cmd.spawn() {
                        tokio::spawn(async move {
                            let _ = child.wait().await;
                        });
                    }
                    let info = self.spawn_session(base).await?;
                    return Ok(Some((term(&info), Some(info))));
                }
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .create(&run)
                    .map_err(|e| KeltaError::internal(format!("runtime dir: {e}")))?;
                let req = SpawnRequest {
                    id: None,
                    kind: SessionKind::Editor { adapter: preset.id.clone() },
                    program: Some(preset.command.clone()),
                    args,
                    ..base
                };
                let info = self.spawn_session_with_id(id, req).await?;
                Ok(Some((term(&info), Some(info))))
            }
            other => {
                let Some(tool) = other.strip_prefix("tool:") else {
                    return Err(KeltaError::invalid(format!("unknown template session `{other}`")));
                };
                let handle = self
                    .plugins
                    .tool_open(project, &ToolId::new(tool), ctx.clone(), Placement::Focused)
                    .await?;
                Ok(Some(match handle {
                    ToolHandle::External => return Ok(None), // launched beside Kelta, no pane
                    ToolHandle::Pty { session_id } => {
                        let info = self.sessions.lock().get(&session_id).map(|e| e.info.clone());
                        (PaneContent::Terminal { session_id }, info)
                    }
                    ToolHandle::Web { instance_id, .. } => {
                        (PaneContent::Web { tool_instance_id: instance_id }, None)
                    }
                }))
            }
        }
    }

    /// `session_spawn_template`.
    pub async fn session_spawn_template(
        &self,
        project: &ProjectId,
        template_id: &str,
        ctx: TemplateCtx,
        placement: Placement,
    ) -> Result<Vec<SessionInfo>, KeltaError> {
        self.rt.capture();
        if !self.project_exists(project) {
            return Err(KeltaError::not_found(format!("project {project}")));
        }
        let s = self.cfg.effective(Some(project));
        let tpl = s
            .session_templates
            .iter()
            .find(|t| t.id == template_id && t.enabled)
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("session template {template_id}")))?;
        let cwd: PathBuf = match &ctx.cwd {
            Some(c) => crate::projects::repo_path(&c.to_string_lossy(), &self.home_dir()),
            None => self.project_cwd(project, ctx.repo_id.as_deref()),
        };
        let repo = self.cfg.project(project).and_then(|p| {
            ctx.repo_id
                .as_deref()
                .and_then(|r| p.repos.iter().find(|x| x.id == r).cloned())
                .or_else(|| p.repos.iter().find(|x| x.primary).cloned())
                .or_else(|| p.repos.first().cloned())
        });
        let mut vars: BTreeMap<&str, String> = BTreeMap::new();
        vars.insert("cwd", cwd.to_string_lossy().into_owned());
        vars.insert("project", project.to_string());
        vars.insert("base", repo.as_ref().map(|r| r.base.clone()).unwrap_or_else(|| "main".into()));
        vars.insert("repo", repo.as_ref().map(|r| r.id.clone()).unwrap_or_default());
        vars.insert("task", String::new()); // plain sessions have no task: `{task}` renders empty
        for (k, v) in &ctx.extra {
            if let Some(key) = ["key", "title", "url"].iter().find(|x| *x == k) {
                vars.insert(key, v.clone());
            }
        }

        let mut ls = Vec::new();
        leaves(&tpl.layout, &mut ls);
        let mut contents = Vec::new();
        let mut spawned = Vec::new();
        let mut first_err = None;
        for leaf in &ls {
            match self.spawn_leaf(leaf, project, &cwd, &ctx, &s, &tpl.id, &vars).await {
                Ok(Some((c, info))) => {
                    contents.push(Some(c));
                    spawned.extend(info);
                }
                Ok(None) => contents.push(None),
                Err(e) => {
                    self.emit(UiEvent::Toast {
                        toast: Toast::error(format!("{}: {}", tpl.label, e.message)),
                    });
                    first_err.get_or_insert(e);
                    contents.push(None);
                }
            }
        }
        let mut it = contents.into_iter();
        let Some(node) = to_layout(&tpl.layout, &mut it) else {
            return Err(first_err
                .unwrap_or_else(|| KeltaError::invalid(format!("template {template_id} spawned nothing"))));
        };
        let first = layout::all_panes(&node).first().map(|(_, c)| (*c).clone()).unwrap_or(PaneContent::Empty);
        let req = OpenPaneRequest {
            content: first,
            placement,
            focus: true,
            tab_title: Some(tpl.label.clone()),
            work_item_id: ctx.work_item_id.clone(),
        };
        self.open_node(project, node, &req)?;
        Ok(spawned)
    }

    /// Session ids of a template-spawned set (helper for callers wanting ids only).
    pub fn ids_of(sessions: &[SessionInfo]) -> Vec<SessionId> {
        sessions.iter().map(|s| s.id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_file_shape() {
        let v = claude_hooks_settings(Path::new("/d/bin/current/kelta-ctl"), &BTreeMap::new());
        let h = &v["hooks"];
        assert_eq!(
            h["Notification"][0]["matcher"],
            "permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input"
        );
        assert_eq!(h["SessionEnd"][0]["hooks"][0].get("async"), None);
        assert_eq!(h["Stop"][0]["hooks"][0]["async"], true);
        assert_eq!(h["Stop"][0]["hooks"][0]["command"], "'/d/bin/current/kelta-ctl' hook");
        let q = claude_hooks_settings(Path::new("/Users/o'brien/kelta-ctl"), &BTreeMap::new());
        assert_eq!(q["hooks"]["Stop"][0]["hooks"][0]["command"], r"'/Users/o'\''brien/kelta-ctl' hook");
    }

    #[test]
    fn claude_argv() {
        let a =
            claude_args(&ClaudeSettings::default(), Some("review"), "U", Path::new("/r/s.json"), Some("hi"));
        assert_eq!(&a[..2], &["--session-id".to_owned(), "U".to_owned()]);
        assert!(a.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "plan"));
        assert_eq!(a.last().map(String::as_str), Some("hi"));
    }
}
