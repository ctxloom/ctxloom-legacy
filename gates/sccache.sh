#!/usr/bin/env bash
# The compiler cache every gate and agent build shares (owner ruling of
# 2026-11-16: each worktree's target/ grew to 120-180G, and three agents
# filled /home): `sccache.sh resolve <version> <dir> <size> <socket>
# <basedirs>` prints what RUSTC_WRAPPER is to be, the absolute path of the
# sccache on PATH, once it has checked that sccache is the pinned
# <version>; and, when the server's socket is not there, starts the server
# with this cache's settings, so the server is not first started by a build
# inside a test gate's memory-cap scope (whose cgroup it would stay in).
# `sccache.sh basedirs <repo>` prints the parent directory of each of the
# repository's worktrees, `:`-joined, sorted: SCCACHE_BASEDIRS' default.
#
# gates/gates.just calls `resolve` while it evaluates its exports (just's
# shell()), so every `just` invocation checks, and a missing or wrong
# sccache FAILS every recipe, before anything runs, saying why and the fix;
# a build never runs uncached unasked. The opt-out is the caller's
# variable (SCCACHE_OPT_OUT names it; ctxloom's is CTXLOOM_SCCACHE=off), in
# which case gates.just never calls this.
#
# The server's settings (sccache reads them when the server starts):
# SCCACHE_DIR=<dir>, SCCACHE_CACHE_SIZE=<size> (an LRU cap), the server's
# unix socket SCCACHE_SERVER_UDS=<socket> (this cache's own server, not a
# default-port one another tool may have started with other settings), and
# SCCACHE_BASEDIRS=<basedirs>. gates.just exports the same, so a server a
# build starts itself (this one idled out) has them too.
#
# Exit: 0; 1 when sccache is missing, not the pinned version, or its server
# cannot be started; 2 on usage.
# Environment: SCCACHE_BIN, the sccache to run (default `sccache` on PATH;
# the self-test's seam), and SCCACHE_OPT_OUT, the opt-out's wording.
# Self-test: scripts/sccache.selftest.sh (`just selftest`).
set -euo pipefail
opt_out=${SCCACHE_OPT_OUT:-CTXLOOM_SCCACHE=off}

case "${1:-}" in
    basedirs)
        [ $# -eq 2 ] || { echo "usage: sccache.sh basedirs <repo>" >&2; exit 2; }
        git -C "$2" worktree list --porcelain | sed -n 's/^worktree //p' |
            while IFS= read -r wt; do dirname "$wt"; done | sort -u | paste -sd: -
        exit 0 ;;
    resolve) [ $# -eq 6 ] || { echo "usage: sccache.sh resolve <version> <dir> <size> <socket> <basedirs>" >&2; exit 2; } ;;
    *) echo "usage: sccache.sh resolve <version> <dir> <size> <socket> <basedirs> | basedirs <repo>" >&2; exit 2 ;;
esac
version=$2 dir=$3 size=$4 socket=$5 basedirs=$6

unable() {
    echo "FAIL sccache: $1 (builds share compiler output through sccache $version; owner ruling of 2026-11-16)" >&2
    echo "  fix: cargo install --locked sccache@$version, or opt out, stating it: $opt_out" >&2
    exit 1
}
bin=$(command -v "${SCCACHE_BIN:-sccache}" 2>/dev/null) || unable "${SCCACHE_BIN:-sccache} is not on PATH"
have=$("$bin" --version 2>/dev/null | head -n 1) || unable "$bin --version failed"
[ "$have" = "sccache $version" ] || unable "$bin is ${have:-of no version}, not the pinned sccache $version"

if [ ! -S "$socket" ]; then
    mkdir -p "$dir" "$(dirname "$socket")"
    # Its own working directory and no inherited output: the server outlives
    # this process (sccache daemonizes it), and must hold no worktree and
    # no caller's pipe.
    err=$(cd / && SCCACHE_DIR=$dir SCCACHE_CACHE_SIZE=$size SCCACHE_SERVER_UDS=$socket \
        SCCACHE_BASEDIRS=$basedirs "$bin" --start-server 2>&1 < /dev/null) \
        || [ -S "$socket" ] || unable "its server would not start: $(head -n 3 <<< "$err")"
fi
echo "$bin"
