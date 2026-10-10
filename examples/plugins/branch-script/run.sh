#!/bin/sh
# Usage: run.sh <script> <branch> <worktree>
# Runs <script> (the `script` setting, a shell command line) with the branch and the worktree as its
# arguments. The screen sets the worktree as the working directory.
set -eu
# An empty or blank setting falls back to `echo`, or `sh -c ' "$@"'` would run the branch name as a command.
script=$1
case $script in *[![:space:]]*) ;; *) script=echo ;; esac
shift
exec sh -c "$script \"\$@\"" run.sh "$@"
