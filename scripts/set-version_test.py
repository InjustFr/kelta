#!/usr/bin/env python3
"""Self-test of scripts/set-version.py on a copy of the versioned files. Run by scripts/qa.sh."""
import os
import shutil
import subprocess
import sys
import tempfile

here = os.path.dirname(os.path.abspath(__file__))
repo = os.path.join(here, "..")
FILES = ["Cargo.toml", "Cargo.lock", "apps/desktop/src-tauri/tauri.conf.json", "ui/package.json",
         "packaging/aur/PKGBUILD", "packaging/homebrew/kelta.rb", "packaging/linux/dev.kelta.Kelta.metainfo.xml"]


def read(root, path):
    with open(os.path.join(root, path), encoding="utf-8") as f:
        return f.read()


with tempfile.TemporaryDirectory() as tmp:
    for p in FILES:
        os.makedirs(os.path.dirname(os.path.join(tmp, p)) or tmp, exist_ok=True)
        shutil.copy(os.path.join(repo, p), os.path.join(tmp, p))
    run = [sys.executable, os.path.join(here, "set-version.py")]
    subprocess.run(run + ["9.8.7", tmp], check=True)
    for p in FILES[:-1]:
        assert "9.8.7" in read(tmp, p), p
    assert '<release version="9.8.7"' in read(tmp, FILES[-1])
    before, after = read(repo, "Cargo.lock").split("\n\n"), read(tmp, "Cargo.lock").split("\n\n")
    changed = [a for b, a in zip(before, after) if a != b]
    assert len(before) == len(after) and changed, "Cargo.lock blocks"
    assert all("\nsource = " not in a and 'version = "9.8.7"' in a for a in changed), "only workspace members"
    assert any('name = "kelta-desktop"' in a for a in changed)
    bad = subprocess.run(run + ["v1", tmp], capture_output=True, text=True)
    assert bad.returncode != 0 and "not a semver" in bad.stderr, bad.stderr
print("set-version_test: ok")
