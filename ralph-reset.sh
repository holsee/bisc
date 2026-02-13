#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Resets the current ralph loop session:
# - Switches back to main
# - Deletes the ralph work branch
# - Cleans up any leftover artifacts
#
# Usage:
#   ./ralph-reset.sh              # delete current ralph/* branch
#   ./ralph-reset.sh my-branch    # delete a specific branch

BRANCH="${1:-}"

# If no branch specified, find the current ralph branch
if [ -z "$BRANCH" ]; then
  CURRENT=$(git branch --show-current 2>/dev/null || true)
  if [[ "$CURRENT" == ralph/* ]]; then
    BRANCH="$CURRENT"
  else
    echo "Not on a ralph branch. Specify one to delete:"
    git branch --list 'ralph/*'
    exit 1
  fi
fi

echo "Resetting ralph session: $BRANCH"

# Switch back to main
git checkout main 2>/dev/null || git checkout master

# Delete the ralph branch
git branch -D "$BRANCH"

# Clean up artifacts
rm -f test.log
rm -rf target/test-logs

echo "Done. Back on $(git branch --show-current)."
