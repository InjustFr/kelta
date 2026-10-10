#!/usr/bin/env python3
"""What the local gate must run for a change (issue #185). Used by scripts/ci-local.sh and scripts/qa.sh.

  affected.py plan <base>     prints shell assignments: rust=all|none|"<pkg> ...", ui=0|1, e2e=0|1, deny=0|1
  affected.py libs <pkg>...   prints the given workspace packages that have a library target (doctests)

Any failure exits non-zero: the caller then runs everything.
"""
import json
import shlex
import subprocess
import sys

# A change here can affect every crate (or the gate itself): full run.
FULL_ALL = ("scripts/",)
FULL_RUST = ("Cargo.lock", "Cargo.toml", ".cargo/", "rust-toolchain", "rustfmt.toml", "clippy.toml", "deny.toml",
             "ui/src/lib/gen/")  # gen/ is codegen output: `codegen --check` must see it
# Outside the workspace members and known to have no Rust effect. Any other path outside a member (schema/,
# examples/, fixtures/ files read by tests, ...) runs every crate: an unknown path costs time, never coverage.
NO_RUST = ("docs/", "ui/", "packages/", "packaging/", "docker/", ".github/", "LICENSE", "package.json",
           "pnpm-lock.yaml", "pnpm-workspace.yaml", "eslint.config.js", ".gitignore")
UI = ("ui/", "packages/", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "eslint.config.js")
DENY = ("Cargo.lock", "deny.toml")
# Tests that run another workspace package's binary, which is not a cargo dependency.
# shortcut: kept by hand; a new test spawning a sibling binary must be added here.
RUNTIME_DEPS = {"kelta-server": {"kelta-ctl"}, "kelta-term": {"tui-sim"}, "kelta-bench": {"tui-sim"}}


def plan(files, dirs, deps):
    """files: changed repo-relative paths; dirs: member name -> repo-relative dir;
    deps: member name -> member names it depends on (any dependency kind)."""
    full_all = any(f.startswith(FULL_ALL) for f in files)
    full_rust = full_all
    changed = set()
    for f in files:
        if f.startswith(FULL_RUST):
            full_rust = True
            continue
        owners = [n for n, d in dirs.items() if f.startswith(d + "/")]
        if owners:
            changed.add(max(owners, key=lambda n: len(dirs[n])))  # nested members: the innermost owns it
        elif not (f.startswith(NO_RUST) or f.endswith(".md")):
            full_rust = True
    rdeps = {n: set() for n in dirs}
    for n, ds in deps.items():
        for d in ds:
            rdeps[d].add(n)
    affected, todo = set(), list(changed)
    while todo:
        n = todo.pop()
        if n not in affected:
            affected.add(n)
            todo.extend(rdeps[n])
    rust = "all" if full_rust else " ".join(sorted(affected)) or "none"
    ui = full_all or any(f.startswith(UI) for f in files)
    e2e = full_all or ui or "kelta-proto" in changed or any(f.startswith("apps/desktop/") for f in files)
    deny = full_all or any(f.startswith(DENY) for f in files)
    return {"rust": rust, "ui": int(ui), "e2e": int(e2e), "deny": int(deny)}


def git_lines(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout.splitlines()


def changed_files(base):
    # --no-renames: a moved file counts for its old and its new package.
    files = git_lines("diff", "--name-only", "--no-renames", f"{base}...HEAD")
    files += git_lines("diff", "--name-only", "--no-renames", "HEAD")
    files += git_lines("ls-files", "--others", "--exclude-standard")
    return sorted(set(files))


def workspace():
    meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--locked"], check=True,
                                     capture_output=True, text=True).stdout)
    root = meta["workspace_root"].rstrip("/") + "/"
    members = {p["id"]: p for p in meta["packages"] if p["id"] in meta["workspace_members"]}
    dirs = {p["name"]: p["manifest_path"].removeprefix(root).rpartition("/")[0] for p in members.values()}
    deps = {members[n["id"]]["name"]: {members[d["pkg"]]["name"] for d in n["deps"] if d["pkg"] in members}
            for n in meta["resolve"]["nodes"] if n["id"] in members}
    for n, extra in RUNTIME_DEPS.items():
        deps[n] |= extra
    return dirs, deps, members


def main(argv):
    if argv[:1] == ["plan"] and len(argv) == 2:
        dirs, deps, _ = workspace()
        for k, v in plan(changed_files(argv[1]), dirs, deps).items():
            print(f"{k}={shlex.quote(str(v))}")
    elif argv[:1] == ["libs"]:
        _, _, members = workspace()
        libs = {p["name"] for p in members.values() if any({"lib", "rlib", "proc-macro"} & set(t["kind"]) for t in p["targets"])}
        unknown = set(argv[1:]) - {p["name"] for p in members.values()}
        if unknown:
            sys.exit(f"affected.py: not workspace packages: {' '.join(sorted(unknown))}")
        print(" ".join(p for p in argv[1:] if p in libs))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
