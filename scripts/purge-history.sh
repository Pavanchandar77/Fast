#!/usr/bin/env bash
# ==============================================================================
# scripts/purge-history.sh
# Purges tracked build artifacts (nexus-os/target/) from Git history.
#
# DO NOT RUN THIS SCRIPT WITHOUT EXPLICIT TEAM / LEAD AUTHORIZATION.
# Rewriting Git history modifies all commit hashes and requires force-pushing.
# ==============================================================================

set -euo pipefail

echo "==================================================================="
echo "⚠️  WARNING: PERMANENT GIT HISTORY REWRITE"
echo "==================================================================="
echo "This script drops 'nexus-os/target/' and 'target/' from all git commits"
echo "using git-filter-repo. All commit hashes will change!"
echo "A force push ('git push --force --all') will be required afterwards."
echo "==================================================================="

# Ensure git-filter-repo is installed
if ! command -v git-filter-repo >/dev/null 2>&1; then
    echo "ERROR: 'git-filter-repo' is not installed."
    echo "Install it via: pip install git-filter-repo  OR  sudo apt install git-filter-repo"
    exit 1
fi

# Ensure clean working tree
if [ -n "$(git status --porcelain)" ]; then
    echo "ERROR: Working tree is not clean. Commit or stash changes first."
    exit 1
fi

read -rp "Are you absolutely sure you want to rewrite git history? [y/N]: " confirm
if [[ "${confirm}" != [yY] && "${confirm}" != [yY][eE][sS] ]]; then
    echo "Aborted by user."
    exit 0
fi

echo "Purging target directories from Git history..."
git filter-repo --path nexus-os/target --path target --invert-paths --force

echo "Shrinking repository and expiring reflogs..."
git reflog expire --expire=now --all
git gc --prune=now --aggressive

echo "✅ History purge complete. Verification:"
git count-objects -vH
