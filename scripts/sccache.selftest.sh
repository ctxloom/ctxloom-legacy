#!/usr/bin/env bash
# Self-test of gates/sccache.sh (or $SCCACHE_SH), the compiler cache's check,
# and of its wiring in the justfile. With sccache missing, or not the pinned
# version, `resolve` FAILS, saying why and the fix (the install line and the
# opt-out), and starts nothing; with the pinned one it prints its absolute
# path and starts its server, with the cache's settings, only when the
# server's socket is not there. `basedirs` is each worktree's parent, sorted,
# `:`-joined. Through just: a missing sccache fails a recipe before it runs,
# with the same fix; CTXLOOM_SCCACHE=off runs it with an empty RUSTC_WRAPPER
# and CARGO_INCREMENTAL=0; another word is refused. A fake sccache (an sh
# script) stands in for the real one.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
sh_=${SCCACHE_SH:-$here/../gates/sccache.sh}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL sccache.selftest: $*" >&2; exit 1; }
while IFS= read -r v; do unset "$v"; done < <(git rev-parse --local-env-vars)

mkdir -p "$tmp/bin"
# The fake: says its version; on --start-server records its settings and
# makes the socket (a unix socket, by python; any socket file will do).
cat > "$tmp/bin/sccache" <<'EOF'
#!/bin/sh
case "$1" in
    --version) echo "sccache ${FAKE_VERSION:-0.19.0}" ;;
    --start-server)
        echo "dir=$SCCACHE_DIR size=$SCCACHE_CACHE_SIZE uds=$SCCACHE_SERVER_UDS basedirs=$SCCACHE_BASEDIRS cwd=$(pwd)" >> "$FAKE_LOG"
        python3 -c 'import socket,sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$SCCACHE_SERVER_UDS" ;;
esac
EOF
chmod +x "$tmp/bin/sccache"
export FAKE_LOG=$tmp/log
dir=$tmp/cache sock=$tmp/cache/server.sock
resolve() { "$sh_" resolve 0.19.0 "$dir" 40G "$sock" /a:/b; }

# Missing: refused, said, the fix the install line and the opt-out.
rc=0
SCCACHE_BIN=no-such-sccache resolve > "$tmp/out" 2>&1 || rc=$?
[ "$rc" -eq 1 ] || fail "missing: exit $rc, not 1: $(cat "$tmp/out")"
grep -q "^FAIL sccache: no-such-sccache is not on PATH" "$tmp/out" || fail "missing: no FAIL line: $(cat "$tmp/out")"
grep -q "^  fix: cargo install --locked sccache@0.19.0, or opt out, stating it: CTXLOOM_SCCACHE=off$" "$tmp/out" \
    || fail "missing: no fix: $(cat "$tmp/out")"
[ ! -e "$FAKE_LOG" ] && [ ! -e "$sock" ] || fail "missing: something was started"

# Another version: refused, naming both.
rc=0
FAKE_VERSION=0.13.0 SCCACHE_BIN=$tmp/bin/sccache resolve > "$tmp/out" 2>&1 || rc=$?
[ "$rc" -eq 1 ] || fail "old version: exit $rc, not 1: $(cat "$tmp/out")"
grep -q "is sccache 0.13.0, not the pinned sccache 0.19.0" "$tmp/out" || fail "old version: not said: $(cat "$tmp/out")"
[ ! -e "$FAKE_LOG" ] || fail "old version: a server was started"

# The opt-out's wording is the caller's.
rc=0
SCCACHE_OPT_OUT=OTHER=off SCCACHE_BIN=no-such-sccache resolve > "$tmp/out" 2>&1 || rc=$?
grep -q "stating it: OTHER=off$" "$tmp/out" || fail "opt-out wording: $(cat "$tmp/out")"

# The pinned one, on PATH: its path, and the server started once, with the
# cache's settings, from /.
out=$(PATH=$tmp/bin:$PATH resolve) || fail "pinned: failed: $out"
[ "$out" = "$tmp/bin/sccache" ] || fail "pinned: printed '$out', not the fake's path"
[ "$(cat "$FAKE_LOG")" = "dir=$dir size=40G uds=$sock basedirs=/a:/b cwd=/" ] || fail "pinned: started as: $(cat "$FAKE_LOG")"
[ -S "$sock" ] || fail "pinned: no socket"
out=$(PATH=$tmp/bin:$PATH resolve) || fail "pinned, running: failed: $out"
[ "$(grep -c '' "$FAKE_LOG")" -eq 1 ] || fail "pinned, running: started again: $(cat "$FAKE_LOG")"

# basedirs: each worktree's parent, sorted, joined.
g() { git -c user.name=t -c user.email=t@t "$@"; }
g init -q "$tmp/p/main"
g -C "$tmp/p/main" commit -q --allow-empty -m one
mkdir -p "$tmp/w"
g -C "$tmp/p/main" worktree add -q --detach "$tmp/w/one"
g -C "$tmp/p/main" worktree add -q --detach "$tmp/w/two"
[ "$("$sh_" basedirs "$tmp/p/main")" = "$tmp/p:$tmp/w" ] || fail "basedirs: $("$sh_" basedirs "$tmp/p/main")"

# Usage.
rc=0; "$sh_" > /dev/null 2>&1 || rc=$?; [ "$rc" -eq 2 ] || fail "usage: exit $rc, not 2"
rc=0; "$sh_" resolve 0.19.0 > /dev/null 2>&1 || rc=$?; [ "$rc" -eq 2 ] || fail "resolve usage: exit $rc, not 2"

# Through the justfile: a recipe fails before it runs when sccache is
# missing; the opt-out runs it with no wrapper; another word is refused.
top=$(cd "$here/.." && pwd)
jf=(just --justfile "$top/justfile" --working-directory "$top")
env -u GITHUB_ACTIONS -u CTXLOOM_SCCACHE SCCACHE_BIN=no-such-sccache CTXLOOM_SCCACHE_DIR=$tmp/xdg \
    "${jf[@]}" show-version > "$tmp/out" 2>&1 && fail "just, missing: the recipe ran: $(cat "$tmp/out")"
grep -q "^  fix: cargo install --locked sccache@0.19.0, or opt out, stating it: CTXLOOM_SCCACHE=off$" "$tmp/out" \
    || fail "just, missing: no fix: $(cat "$tmp/out")"
for v in RUSTC_WRAPPER CARGO_INCREMENTAL; do
    got=$(CTXLOOM_SCCACHE=off SCCACHE_BIN=no-such-sccache "${jf[@]}" --evaluate "$v") || fail "just, off: $v failed"
    case "$v=$got" in RUSTC_WRAPPER= | CARGO_INCREMENTAL=0) ;; *) fail "just, off: $v is '$got'" ;; esac
done
env -u GITHUB_ACTIONS CTXLOOM_SCCACHE=maybe "${jf[@]}" --evaluate RUSTC_WRAPPER > "$tmp/out" 2>&1 \
    && fail "just, CTXLOOM_SCCACHE=maybe: accepted"
grep -q "sccache_mode is on or off, not 'maybe'" "$tmp/out" || fail "just, maybe: not said: $(cat "$tmp/out")"
got=$(env -u CTXLOOM_SCCACHE GITHUB_ACTIONS=true SCCACHE_BIN=no-such-sccache "${jf[@]}" --evaluate RUSTC_WRAPPER) \
    || fail "just, on GitHub Actions: failed"
[ -z "$got" ] || fail "just, on GitHub Actions: RUSTC_WRAPPER is '$got', not off"

echo "ok   sccache.selftest (missing and other versions refused with the fix; server started once; basedirs; just's wiring and opt-out)"
