#!/usr/bin/env bash
# Self-test of the mutation run's memory cap, gates/memory-cap.sh (or
# $MEMORY_CAP). Checks: with no user bus to reach it FAILS, saying why and
# the fix, and runs nothing (the command, and `--check`); the opt-out `none`
# runs the command uncapped and says so; usage is refused. Where a systemd
# user bus is reachable, also: the command runs inside a scope whose
# memory.max is the cap, its exit code is the wrapper's, and a process of it
# that outgrows the cap is OOM-killed while the run itself goes on
# (OOMPolicy=continue), the peak and the kill said. Without a bus that part
# says it skips.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
cap=${MEMORY_CAP:-$here/../gates/memory-cap.sh}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL memory-cap.selftest: $*" >&2; exit 1; }

# No bus: no XDG_RUNTIME_DIR, no DBUS_SESSION_BUS_ADDRESS, and an empty
# runtime directory in place of /run/user.
nobus=(env -u XDG_RUNTIME_DIR -u DBUS_SESSION_BUS_ADDRESS "MEMORY_CAP_RUN_USER=$tmp/run-user")
mkdir -p "$tmp/run-user/$(id -u)"
rc=0
"${nobus[@]}" "$cap" probe 1G 0 touch "$tmp/ran" > "$tmp/out" 2>&1 || rc=$?
[ "$rc" -eq 1 ] || fail "no bus: exit $rc, not 1: $(cat "$tmp/out")"
[ ! -e "$tmp/ran" ] || fail "no bus: the command ran uncapped"
grep -q "^FAIL probe: no memory cap: no systemd user bus" "$tmp/out" || fail "no bus: no FAIL line: $(cat "$tmp/out")"
grep -q "^  fix: .*MUTANTS_MEMORY_MAX=none" "$tmp/out" || fail "no bus: no fix naming the opt-out: $(cat "$tmp/out")"
rc=0
"${nobus[@]}" "$cap" probe 1G 0 --check > "$tmp/out" 2>&1 || rc=$?
[ "$rc" -eq 1 ] || fail "no bus, --check: exit $rc, not 1: $(cat "$tmp/out")"
grep -q "^FAIL probe: no memory cap" "$tmp/out" || fail "no bus, --check: no FAIL line: $(cat "$tmp/out")"

# The opt-out: uncapped, said, the command's own exit.
rc=0
"${nobus[@]}" "$cap" probe none 0 bash -c 'touch "$1"; exit 7' opt "$tmp/ran" > "$tmp/out" 2>&1 || rc=$?
[ "$rc" -eq 7 ] || fail "opt-out: exit $rc, not the command's 7: $(cat "$tmp/out")"
[ -e "$tmp/ran" ] || fail "opt-out: the command did not run"
grep -q "^probe: memory cap: none (MUTANTS_MEMORY_MAX=none): this run is UNCAPPED" "$tmp/out" \
    || fail "opt-out: not said: $(cat "$tmp/out")"
rm -f "$tmp/ran"
"${nobus[@]}" "$cap" probe none 0 --check > "$tmp/out" 2>&1 || fail "opt-out, --check: failed: $(cat "$tmp/out")"
grep -q "UNCAPPED" "$tmp/out" || fail "opt-out, --check: not said: $(cat "$tmp/out")"

# Usage.
rc=0
"$cap" probe 1G > "$tmp/out" 2>&1 || rc=$?
[ "$rc" -eq 2 ] || fail "usage: exit $rc, not 2"

# A real scope, where this machine has a user bus.
if ! "$cap" probe 1G 0 --check > "$tmp/out" 2>&1; then
    echo "SKIP memory-cap.selftest (the capped scope): no systemd user scope here: $(head -n 1 "$tmp/out")"
    echo "ok   memory-cap.selftest"
    exit 0
fi
# shellcheck disable=SC2016
rc=0
"$cap" probe 64M 0 bash -c 'cat "/sys/fs/cgroup$(sed -n "s/^0:://p" /proc/self/cgroup)/memory.max"; exit 5' \
    > "$tmp/out" 2>&1 || rc=$?
[ "$rc" -eq 5 ] || fail "scope: exit $rc, not the command's 5: $(cat "$tmp/out")"
grep -qx "$((64 * 1024 * 1024))" "$tmp/out" || fail "scope: memory.max is not the 64M cap: $(cat "$tmp/out")"
grep -q "^probe: memory peak .* (cap MemoryMax=64M MemorySwapMax=0)" "$tmp/out" || fail "scope: no peak line: $(cat "$tmp/out")"
# A child outgrows the cap: it alone is killed, and the run goes on to its
# own exit.
rc=0
"$cap" probe 64M 0 bash -c 'head -c 256M /dev/zero | tail > /dev/null; echo "child $?"; echo after; exit 3' \
    > "$tmp/out" 2>&1 || rc=$?
[ "$rc" -eq 3 ] || fail "OOM: exit $rc, not the run's own 3 (the scope was stopped?): $(cat "$tmp/out")"
grep -qx "child 137" "$tmp/out" || fail "OOM: the child was not killed: $(cat "$tmp/out")"
grep -qx "after" "$tmp/out" || fail "OOM: the run did not go on: $(cat "$tmp/out")"
grep -q "the cap's OOM killer killed 1 process(es) of the run" "$tmp/out" || fail "OOM: the kill not said: $(cat "$tmp/out")"
echo "ok   memory-cap.selftest"
