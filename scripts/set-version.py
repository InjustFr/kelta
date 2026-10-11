#!/usr/bin/env python3
"""Sets the release version everywhere it is written down (semantic-release's prepare step, .releaserc.json).
Usage: set-version.py <version> [root]   (root defaults to the repo; scripts/set-version_test.py passes a copy)"""
import datetime
import os
import re
import sys


def sub(root, path, pattern, repl, flags=re.M):
    p = os.path.join(root, path)
    with open(p, encoding="utf-8") as f:
        text = f.read()
    new, n = re.subn(pattern, repl, text, count=1, flags=flags)
    if n != 1:
        sys.exit(f"set-version: {path}: no match for {pattern}")
    with open(p, "w", encoding="utf-8") as f:
        f.write(new)


def main(version, root):
    if not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?", version):
        sys.exit(f"set-version: not a semver version: {version}")
    sub(root, "Cargo.toml", r'^version = ".*"$', f'version = "{version}"')
    sub(root, "apps/desktop/src-tauri/tauri.conf.json", r'"version": "[^"]*"', f'"version": "{version}"')
    sub(root, "ui/package.json", r'"version": "[^"]*"', f'"version": "{version}"')
    sub(root, "packaging/aur/PKGBUILD", r"^pkgver=.*$", f"pkgver={version}")
    sub(root, "packaging/homebrew/kelta.rb", r'^  version ".*"$', f'  version "{version}"')
    today = datetime.datetime.now(datetime.timezone.utc).date().isoformat()
    sub(root, "packaging/linux/dev.kelta.Kelta.metainfo.xml", r"<releases>\n",
        f'<releases>\n    <release version="{version}" date="{today}" />\n')
    # Cargo.lock: the workspace members are the packages without a `source` (every member uses version.workspace).
    lock = os.path.join(root, "Cargo.lock")
    with open(lock, encoding="utf-8") as f:
        blocks = f.read().split("\n\n")
    for i, b in enumerate(blocks):
        if b.startswith("[[package]]") and "\nsource = " not in b:
            blocks[i] = re.sub(r'^version = ".*"$', f'version = "{version}"', b, count=1, flags=re.M)
    with open(lock, "w", encoding="utf-8") as f:
        f.write("\n\n".join(blocks))


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3):
        sys.exit("usage: set-version.py <version> [root]")
    main(sys.argv[1], sys.argv[2] if len(sys.argv) == 3 else os.path.join(os.path.dirname(__file__), ".."))
