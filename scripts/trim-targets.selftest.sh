#!/usr/bin/env bash
# Self-test of scripts/trim-targets.sh (or $TRIM_TARGETS) against a fixture
# repository with a worktree of each kind, each holding a target/: one
# landed (its HEAD in the base), whose target/ alone is removed; one not
# landed (a commit the base lacks); one with a tracked change; one with an
# untracked file; one a running process uses (its working directory); one
# git has locked. Every kept one is said with its reason, its target/ and
# sources untouched, and no worktree or git record is removed. Also: a
# worktree with no target/ is skipped and said; an unknown base and usage
# are refused, removing nothing.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
trim=${TRIM_TARGETS:-$here/trim-targets.sh}
tmp=$(mktemp -d)
sleeper=
cleanup() { [ -z "$sleeper" ] || kill "$sleeper" 2>/dev/null || true; rm -rf "$tmp"; }
trap cleanup EXIT
fail() { echo "FAIL trim-targets.selftest: $*" >&2; exit 1; }
while IFS= read -r v; do unset "$v"; done < <(git rev-parse --local-env-vars)
g() { git -c user.name=t -c user.email=t@t -c init.defaultBranch=integ "$@"; }

repo=$tmp/repo
g init -q "$repo"
printf 'target/\n' > "$repo/.gitignore"
echo one > "$repo/file"
g -C "$repo" add .gitignore file
g -C "$repo" commit -q -m one
for name in landed unlanded dirty untracked inuse locked notarget; do
    g -C "$repo" worktree add -q --detach "$tmp/$name" integ
done
for name in landed unlanded dirty untracked inuse locked; do
    mkdir -p "$tmp/$name/target/debug"
    echo build > "$tmp/$name/target/debug/out"
done
echo two > "$tmp/unlanded/file"
g -C "$tmp/unlanded" commit -q -am two
echo changed > "$tmp/dirty/file"
echo new > "$tmp/untracked/new-source"
g -C "$repo" worktree lock "$tmp/locked"
(cd "$tmp/inuse" && exec sleep 300) &
sleeper=$!
# Wait, bounded, until the sleeper's working directory is the worktree.
for _ in $(seq 50); do
    [ "$(readlink "/proc/$sleeper/cwd" 2>/dev/null)" = "$tmp/inuse" ] && break
    sleep 0.1
done

has() { grep -qF -- "$1" <<< "$out" || fail "no line '$1' in: $out"; }
# A dry run judges the same and removes nothing.
out=$(TRIM_TARGETS_REPO=$repo "$trim" --dry-run integ 2>&1) || fail "dry run: exit $?: $out"
has "would remove $tmp/landed/target ("
has "trim-targets: (dry run) would have removed 1 target/ directories, kept 5"
[ -f "$tmp/landed/target/debug/out" ] || fail "the dry run removed something"

out=$(TRIM_TARGETS_REPO=$repo "$trim" integ 2>&1) || fail "exit $?: $out"
has "removed $tmp/landed/target ("
has "kept    $tmp/unlanded: HEAD "
has "is not in integ (not landed)"
has "kept    $tmp/dirty: tracked changes (1 files)"
has "kept    $tmp/untracked: untracked files (1, work not yet added)"
has "kept    $tmp/inuse: in use by pid $sleeper (sleep)"
has "kept    $tmp/locked: locked (git worktree lock)"
has "skip    $tmp/notarget: no target/"
has "skip    $repo: no target/"
has "trim-targets: removed 1 target/ directories, kept 5 (base integ, "
[ ! -e "$tmp/landed/target" ] || fail "the landed worktree's target/ is still there"
[ -f "$tmp/landed/file" ] || fail "the landed worktree's source was removed"
for name in unlanded dirty untracked inuse locked; do
    [ -f "$tmp/$name/target/debug/out" ] || fail "$name: its target/ was touched"
done
[ -f "$tmp/untracked/new-source" ] && [ "$(cat "$tmp/dirty/file")" = changed ] || fail "work was touched"
[ "$(g -C "$repo" worktree list | grep -c '')" -eq 8 ] || fail "a worktree record went: $(g -C "$repo" worktree list)"

# Once the process is gone, that worktree is trimmed too; the rest stay.
kill "$sleeper"; wait "$sleeper" 2>/dev/null || true; sleeper=
out=$(TRIM_TARGETS_REPO=$repo "$trim" integ 2>&1) || fail "second run: exit $?: $out"
has "removed $tmp/inuse/target ("
has "trim-targets: removed 1 target/ directories, kept 4"

# An unknown base, and usage: refused, nothing removed.
rc=0
out=$(TRIM_TARGETS_REPO=$repo "$trim" no-such-base 2>&1) || rc=$?
[ "$rc" -eq 1 ] || fail "unknown base: exit $rc, not 1: $out"
has "FAIL trim-targets: base 'no-such-base' is not a commit"
rc=0
"$trim" > /dev/null 2>&1 || rc=$?
[ "$rc" -eq 2 ] || fail "usage: exit $rc, not 2"
[ -f "$tmp/dirty/target/debug/out" ] || fail "a refused run removed something"

echo "ok   trim-targets.selftest (landed removed, dry run removes nothing; unlanded, dirty, untracked, in use, locked kept and said)"
