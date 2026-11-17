#!/usr/bin/env bash
# Clear the target/ of every worktree of this repository that has landed and
# holds nothing: `trim-targets.sh [--dry-run] <base>` (`just trim-targets
# [base] [--dry-run]`, owner
# ruling of 2026-11-16: each worktree's target/ grew to 120-180G and three
# agents' worktrees filled /home).
#
# A worktree's target/ is removed only when ALL of these hold:
#   - its HEAD is reachable from <base> (landed; default, in the justfile,
#     origin/daemon/integration);
#   - no running process uses it: none has its working directory, its
#     executable or an open file under the worktree (read from /proc; a
#     process this user cannot read is not seen);
#   - it has no tracked change (staged or not), no untracked file that is not
#     ignored (work not yet added), and git has not locked it.
# Anything else is kept, and said why. Only target/ is removed: never a
# source file, never the worktree, never its git record. A worktree with no
# target/ is said and skipped. One line per worktree:
#
#   removed <worktree>/target (<size>)
#   kept    <worktree>: <why>
#   skip    <worktree>: no target/
#
# then a summary line. `--dry-run` judges the same and removes nothing,
# saying `would remove` for each it would. Without /proc (macOS) it cannot tell which processes
# use a worktree, so it FAILS and removes nothing.
#
# Exit: 0; 1 when it cannot judge (no /proc, an unknown base); 2 on usage.
# Environment: TRIM_TARGETS_REPO, the repository whose worktrees are judged
# (default: the one holding this script; the self-test's seam).
# Self-test: scripts/trim-targets.selftest.sh (`just selftest`).
set -euo pipefail
dry=
[ "${1:-}" = --dry-run ] && { dry=1; shift; }
[ $# -eq 1 ] || { echo "usage: trim-targets.sh [--dry-run] <base>" >&2; exit 2; }
base=$1
here=$(cd "$(dirname "$0")" && pwd)
repo=${TRIM_TARGETS_REPO:-$here/..}
# Unset git's per-repository variables (a hook's GIT_DIR, GIT_INDEX_FILE),
# so each `git -C <worktree>` reads that worktree.
while IFS= read -r v; do unset "$v"; done < <(git rev-parse --local-env-vars)
# Out of every worktree, so this process uses none of them.
cd /

[ -d /proc/self/fd ] || { echo "FAIL trim-targets: no /proc: cannot tell which processes use a worktree, so nothing is removed" >&2; exit 1; }
base_sha=$(git -C "$repo" rev-parse --verify --quiet "$base^{commit}") \
    || { echo "FAIL trim-targets: base '$base' is not a commit of $repo (fetch it?)" >&2; exit 1; }

# Every path a running process uses: its cwd, its executable and its open
# files, one per line (a process gone in between, or not ours, says nothing).
used=$(
    for p in /proc/[0-9]*; do
        pid=${p#/proc/}
        [ "$pid" = $$ ] && continue
        for l in "$p/cwd" "$p/exe" "$p"/fd/*; do
            t=$(readlink "$l" 2>/dev/null) || continue
            printf '%s\t%s\n' "$pid" "$t"
        done
    done 2>/dev/null || true
)

# The process (pid and name) using a path under $1, if any.
user_of() {
    local hit pid
    hit=$(awk -F'\t' -v d="$1" '$2 == d || index($2, d "/") == 1 { print $1; exit }' <<< "$used")
    [ -n "$hit" ] || return 1
    pid=$hit
    echo "pid $pid ($(cat "/proc/$pid/comm" 2>/dev/null || echo gone))"
}

removed=0 kept=0
worktrees=()
locked=()
cur=
while IFS= read -r line; do
    case "$line" in
        "worktree "*) cur=${line#worktree }; worktrees+=("$cur") ;;
        locked*) locked+=("$cur") ;;
    esac
done < <(git -C "$repo" worktree list --porcelain)

for wt in "${worktrees[@]}"; do
    if [ ! -d "$wt/target" ]; then
        echo "skip    $wt: no target/"
        continue
    fi
    why=
    for l in ${locked[@]+"${locked[@]}"}; do [ "$l" = "$wt" ] && why="locked (git worktree lock)"; done
    if [ -z "$why" ]; then
        head=$(git -C "$wt" rev-parse --verify --quiet HEAD) || why="no HEAD"
    fi
    if [ -z "$why" ] && ! git -C "$repo" merge-base --is-ancestor "$head" "$base_sha"; then
        why="HEAD ${head:0:9} is not in $base (not landed)"
    fi
    if [ -z "$why" ] && u=$(user_of "$wt"); then
        why="in use by $u"
    fi
    if [ -z "$why" ]; then
        status=$(git -C "$wt" status --porcelain=v1 2>/dev/null) || why="git status failed"
        if [ -z "$why" ] && grep -q '^[^?]' <<< "$status"; then
            why="tracked changes ($(grep -c '^[^?]' <<< "$status") files)"
        elif [ -z "$why" ] && grep -q '^??' <<< "$status"; then
            why="untracked files ($(grep -c '^??' <<< "$status"), work not yet added)"
        fi
    fi
    if [ -n "$why" ]; then
        echo "kept    $wt: $why"
        kept=$((kept + 1))
        continue
    fi
    size=$(du -sh "$wt/target" 2>/dev/null | cut -f1)
    if [ -n "$dry" ]; then
        echo "would remove $wt/target (${size:-?})"
    else
        rm -rf "$wt/target"
        echo "removed $wt/target (${size:-?})"
    fi
    removed=$((removed + 1))
done
echo "trim-targets: ${dry:+(dry run) would have }removed $removed target/ directories, kept $kept (base $base, ${base_sha:0:9})"
