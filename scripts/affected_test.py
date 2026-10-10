#!/usr/bin/env python3
"""Self-test of scripts/affected.py plan() on a fixture workspace. Run by scripts/qa.sh."""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from affected import plan  # noqa: E402

# kelta-proto <- core <- desktop (nested under apps/desktop), kelta-proto <- xtask (dev-dep counts too); term alone.
DIRS = {"kelta-proto": "crates/kelta-proto", "core": "crates/core", "term": "crates/term",
        "desktop": "apps/desktop/src-tauri", "xtask": "xtask", "sim": "fixtures/sim",
        "kelta-term": "crates/kelta-term", "kelta-plugins": "crates/kelta-plugins"}
DEPS = {"kelta-proto": set(), "core": {"kelta-proto"}, "term": set(), "desktop": {"core", "term"},
        "xtask": {"kelta-proto"}, "sim": set(), "kelta-term": set(), "kelta-plugins": set()}

CASES = [
    # (changed files, rust, ui, e2e, deny)
    ([], "none", 0, 0, 0),
    (["docs/SPEC.md", "README.md", "crates/core/README.md"], "core desktop", 0, 0, 0),
    (["docs/SPEC.md", "CHANGELOG.md", ".github/workflows/ci.yml"], "none", 0, 0, 0),
    (["crates/term/src/lib.rs"], "desktop term", 0, 0, 0),
    (["crates/kelta-proto/src/lib.rs"], "core desktop kelta-proto xtask", 0, 1, 0),
    (["apps/desktop/src-tauri/src/main.rs"], "desktop", 0, 1, 0),
    (["apps/desktop/README.txt"], "all", 0, 1, 0),  # outside the member: unknown, so every crate
    (["fixtures/sim/src/main.rs"], "sim", 0, 0, 0),
    (["fixtures/recordings/x.ansi", "ui/src/App.svelte"], "all", 1, 1, 0),
    (["schema/settings.schema.json"], "all", 0, 0, 0),
    (["ui/src/App.svelte"], "none", 1, 1, 0),
    (["ui/src/lib/gen/Foo.ts"], "all", 1, 1, 0),
    (["packages/plugin-sdk/src/index.ts"], "none", 1, 1, 0),
    (["packaging/build.sh", "docker/ubuntu-build.Dockerfile"], "none", 0, 0, 0),
    (["docs/contracts/terminal-queries.md"], "kelta-term", 0, 0, 0),  # read by a kelta-term test
    (["ui/tests/e2e/plugins/hello-screen.preview.json"], "kelta-plugins", 1, 1, 0),  # read by a kelta-plugins test
    (["Cargo.lock"], "all", 0, 0, 1),
    (["deny.toml"], "all", 0, 0, 1),
    (["Cargo.toml"], "all", 0, 0, 0),
    (["crates/term/Cargo.toml"], "desktop term", 0, 0, 0),
    ([".cargo/config.toml"], "all", 0, 0, 0),
    (["rust-toolchain.toml"], "all", 0, 0, 0),
    (["clippy.toml"], "all", 0, 0, 0),
    (["scripts/qa.sh"], "all", 1, 1, 1),
    (["crates/termx/src/lib.rs"], "all", 0, 0, 0),  # prefix of a member dir is not that member
]

fails = 0
for files, rust, ui, e2e, deny in CASES:
    want = {"rust": rust, "ui": ui, "e2e": e2e, "deny": deny}
    got = plan(files, DIRS, DEPS)
    if got != want:
        fails += 1
        print(f"affected_test: {files}: want {want}, got {got}", file=sys.stderr)
sys.exit(1 if fails else 0)
