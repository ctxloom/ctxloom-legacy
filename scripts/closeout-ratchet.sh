#!/usr/bin/env bash
# The close-out of a slice landing on an integration branch that carries a red
# spec on purpose (owner ruling 165): `just closeout-ratchet <base>`. The
# gates of `just closeout` (build, lint, lint-deps, test, lint-os,
# mutants-batch), each as usual, but for one thing: the feature runner (its
# cluster tests, `tests::gherkin_features::clusters::*`) may fail, so long
# as the scenarios red on the slice are a SUBSET of those red on the base.
# A slice may turn red green; it may never add a red scenario or break a
# green one (scripts/ratchet-judge.sh).
#
#   1. The base's list: `just ratchet-base <base>` (scripts/ratchet-base.sh,
#      the feature runner on the base's tip in a throwaway worktree, cached by
#      its sha). Without it nothing can be judged: FAIL, exit 1, at once.
#   2. Every gate runs, so one report shows everything that is red. `test` is
#      `just test` with GHERKIN_RATCHET naming target/logs/ratchet/slice.list:
#      the feature runner writes its scenario list there and passes; every
#      other test fails `test` as it always does. The list is then judged
#      (the `ratchet` line), each scenario whose colour differs from the
#      base's list first run again on the base, alone and in its feature
#      (`ratchet-base.sh <base> --reconfirm`): a difference the base does not
#      reproduce is FLAKY and fails the ratchet (scripts/ratchet-judge.sh).
#      `mutants-batch` mutation-tests
#      merge-base(<base>, HEAD)..HEAD with GHERKIN_RATCHET set too, so the
#      base's red scenarios do not make its baseline red.
#   3. A summary line: `ok   closeout-ratchet (6 gates; base 1a2b3c4: 136 red,
#      4 turned green) 812s`, or `FAIL closeout-ratchet: ...`.
#
# On a base with no red scenario (main) the ratchet is the plain closeout: the
# slice may have none either.
#
# `--no-mutation` before the base is the landing gate, `just land-ratchet
# <base>` (owner ruling of 2026-11-13): every gate above but mutants-batch
# (dropped even from a RATCHET_GATES that names it), the ratchet judged the
# same, every line said as `land-ratchet`. It runs at each branch's landing on
# the integration branch; the full closeout-ratchet runs once per batch and
# before main.
#
# Exit: 0 when every gate passed and no scenario turned red; 1 when any gate
# failed, a scenario turned red or the base's list could not be made; 2 on a
# usage error.
#
# Environment: RATCHET_GATES (space-separated recipe names, default the six
# above), RATCHET_BASE_LIST (the base's list, instead of `ratchet-base`) and
# RATCHET_RECONFIRM (a command given `<base> <ids> <out>`, instead of
# `ratchet-base.sh --reconfirm`): the self-test's seams; the mutants-batch
# variables.
set -uo pipefail
top=$(git rev-parse --show-toplevel)
here=$(cd "$(dirname "$0")" && pwd)
name=closeout-ratchet mutation=1
if [ "${1:-}" = --no-mutation ]; then
    name=land-ratchet mutation=
    shift
fi
if [ -z "${1:-}" ]; then
    echo "FAIL $name: usage: $name <base> (the integration branch the slice lands on)" >&2
    exit 2
fi
base=$1
base_sha=$(git -C "$top" rev-parse --verify --quiet "$base^{commit}") || { echo "FAIL $name: $base is not a commit" >&2; exit 2; }
fork=$(git -C "$top" merge-base "$base_sha" HEAD) || { echo "FAIL $name: $base and HEAD have no merge-base" >&2; exit 2; }
read -ra listed <<< "${RATCHET_GATES:-build lint lint-deps test lint-os mutants-batch}"
gates=()
for gate in "${listed[@]}"; do
    [ -z "$mutation" ] && [ "$gate" = mutants-batch ] && continue
    gates+=("$gate")
done
start=$(date +%s)

base_list=${RATCHET_BASE_LIST:-$top/target/ratchet/$base_sha.list}
if [ -z "${RATCHET_BASE_LIST:-}" ]; then
    if ! just --justfile "$top/justfile" ratchet-base "$base_sha" 2>&1; then
        echo "FAIL $name: the base's scenario list could not be made, so nothing can be judged" >&2
        exit 1
    fi
fi
dir=$top/target/logs/ratchet
mkdir -p "$dir"
slice_list=$dir/slice.list
rm -f "$slice_list" "$dir/mutants.list"

# The ratchet's verdict, each difference from the base's list re-confirmed
# on the base first (owner, 2026-11-14): `ratchet-base.sh --reconfirm`, or
# RATCHET_RECONFIRM (the self-test's seam, given the same arguments); with
# RATCHET_BASE_LIST and no RATCHET_RECONFIRM nothing is re-confirmed.
judge_ratchet() {
    local ids=$dir/differ.ids reconfirmed=$dir/reconfirmed.list
    rm -f "$reconfirmed" "$reconfirmed.failures"
    if [ -s "$slice_list" ]; then
        { LC_ALL=C comm -12 <(sed -n 's/^red\t//p' "$slice_list" | LC_ALL=C sort -u) \
              <(sed -n 's/^green\t//p' "$base_list" | LC_ALL=C sort -u)
          LC_ALL=C comm -12 <(sed -n 's/^green\t//p' "$slice_list" | LC_ALL=C sort -u) \
              <(sed -n 's/^red\t//p' "$base_list" | LC_ALL=C sort -u); } | LC_ALL=C sort -u > "$ids"
    else
        : > "$ids"
    fi
    local reconfirm=${RATCHET_RECONFIRM:-}
    if [ -z "$reconfirm" ] && [ -n "${RATCHET_BASE_LIST:-}" ]; then
        "$here/ratchet-judge.sh" "$base_list" "$slice_list" 2>&1
        return
    fi
    : > "$reconfirmed"
    if [ -s "$ids" ]; then
        local run=("$here/ratchet-base.sh" "$base_sha" --reconfirm)
        [ -z "$reconfirm" ] || run=("$reconfirm" "$base_sha")
        if ! "${run[@]}" "$ids" "$reconfirmed" 2>&1; then
            echo "FAIL ratchet: the base could not re-confirm the $(grep -c '' "$ids") scenarios whose colour differs (full log: target/logs/ratchet-base.log)" >&2
            return 1
        fi
    fi
    "$here/ratchet-judge.sh" "$base_list" "$slice_list" "$reconfirmed" 2>&1
}

failed=()
for gate in "${gates[@]}"; do
    case "$gate" in
        test)
            rc=0
            GHERKIN_RATCHET=$slice_list just --justfile "$top/justfile" test 2>&1 || rc=$?
            [ "$rc" -eq 0 ] || failed+=(test)
            # A run that did not build lists nothing: its failure is the test
            # gate's alone.
            if [ "$rc" -eq 0 ] || [ -e "$slice_list" ]; then
                judge_ratchet || failed+=(ratchet)
            fi
            ;;
        mutants-batch)
            GHERKIN_RATCHET=$dir/mutants.list just --justfile "$top/justfile" mutants-batch "$fork" 2>&1 \
                || failed+=(mutants-batch)
            ;;
        *)
            just --justfile "$top/justfile" "$gate" 2>&1 || failed+=("$gate")
            ;;
    esac
done
secs=$(($(date +%s) - start))
red_count() { { grep -c '^red' "$1" 2>/dev/null || true; } | head -n 1; }
fixed=0
if [ -s "$slice_list" ]; then
    fixed=$(LC_ALL=C comm -23 <(sed -n 's/^red\t//p' "$base_list" | LC_ALL=C sort -u) \
        <(sed -n 's/^red\t//p' "$slice_list" | LC_ALL=C sort -u) | grep -c '' || true)
fi
if [ "${#failed[@]}" -eq 0 ]; then
    printf 'ok   %s (%d gates; base %s: %s red, %s turned green) %ds\n' \
        "$name" "${#gates[@]}" "${base_sha:0:7}" "$(red_count "$base_list")" "$fixed" "$secs"
    exit 0
fi
printf 'FAIL %s: %d failed: %s (base %s; logs: target/logs/)\n' \
    "$name" "${#failed[@]}" "${failed[*]}" "${base_sha:0:7}" >&2
exit 1
