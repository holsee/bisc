#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Ralph Wiggum Loop — agentic task runner for bisc
#
# Launches Claude Code interactively on a work branch, then prints
# the ralph-loop command to paste. You watch it work in the TUI.
#
# Usage:
#   ./ralph.sh              # run with defaults (10 iterations)
#   ./ralph.sh 5            # run with custom max iterations
#   ./ralph.sh 5 my-branch  # custom iterations + custom branch name

MAX_ITERATIONS="${1:-10}"
BRANCH="${2:-ralph/$(date +%Y%m%d-%H%M%S)}"

# Ensure working tree is clean before branching
if ! git diff --quiet HEAD 2>/dev/null; then
  echo "error: uncommitted changes. commit or stash before running." >&2
  exit 1
fi

# Create and switch to the work branch
git checkout -b "$BRANCH"
echo "Working on branch: $BRANCH"
echo ""
echo "Claude Code will open interactively. Paste this command:"
echo ""
echo '/ralph-loop:ralph-loop "Read CLAUDE.md for your full workflow. Pick the next eligible task from tasks/TODO/ (lowest number whose dependencies are all in tasks/DONE/), implement it, validate with cargo build/fmt/clippy/test, review test.log for warnings/errors, verify all acceptance criteria, move the task file to tasks/DONE/, commit as BISC-XXX, record any learnings, then pick the next task. When all tasks in tasks/TODO/ are complete, output: <promise>ALL_TASKS_COMPLETE</promise>"'" --max-iterations ${MAX_ITERATIONS} --completion-promise \"ALL_TASKS_COMPLETE\""
echo ""

exec claude --dangerously-skip-permissions
