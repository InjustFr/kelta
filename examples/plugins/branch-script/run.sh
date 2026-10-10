#!/bin/sh
# Usage: run.sh <script> <branch> <worktree>
# Runs <script> (the `script` setting, a shell command line) with the branch and the worktree as its
# arguments. The screen sets the worktree as the working directory.
set -eu
# An empty setting falls back to `echo`, or `sh -c ' "$@"'` would run the branch name as a command.
script=${1:-echo}
shift
exec sh -c "$script \"\$@\"" run.sh "$@"
