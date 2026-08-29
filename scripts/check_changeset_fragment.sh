#!/usr/bin/env bash
# Changeset gate: a PR touching `crates/` must ADD a `changelog.d/<PR>-<slug>.md`
# fragment (see `changelog.d/README.md`; fragments fold into the GitHub Release
# notes at tag time via `scripts/cut_release_notes.sh`, and `CHANGELOG.md` is
# frozen).
#
# Fragment names are PR-keyed so in-flight PRs cannot collide. Getting the
# number wrong is SILENT: nothing reads the fragment until a release is cut,
# and it then attributes the change to a different PR. #8978 recorded four in
# one day, all caught by eye rather than by any gate.
#
# WHY THE NUMBER IS NOT ENFORCED STRICTLY
# ---------------------------------------
# "The fragment must carry this PR's number" is a false positive on two
# legitimate shapes:
#
#   * a backfill -- #8973 renamed three fragments to `8944-`, `8947-` and
#     `8291-`, none of them its own number, so a strict rule would block the
#     very PR that repairs the problem;
#   * a stacked PR whose fragment names the parent change.
#
# So a mismatched real number is a WARNING. An all-zero prefix is different and
# is a hard failure: no PR is ever number 0, so `0000-` is always the unfilled
# placeholder, never a deliberate choice. That alone would have caught three of
# #8978's four cases.
set -euo pipefail

usage() {
    echo "usage: $0 <owner/repo> <pr-number>" >&2
    echo "       $0 --self-test" >&2
}

# Verdict over a `pulls/<n>/files` payload. `--paginate` emits one JSON array
# per page, so every read slurps (`-s`) and flattens (`[.[][]]`).
#
# `jq -e` with no pipe, and `grep` reading a FILE: `jq | grep -q` under
# `pipefail` can fail on SIGPIPE when grep closes early on its first match.
changeset_verdict() {
    local files="$1" pr="$2" added
    added="$(dirname "$files")/added-fragments.txt"

    if ! jq -s -e '[.[][]] | any(.filename | startswith("crates/"))' "$files" >/dev/null; then
        echo "No crates/ changes - gate not applicable."
        return 0
    fi

    jq -s -r '[.[][]]
              | map(select(.status == "added"
                           and (.filename | test("^changelog\\.d/[0-9]+-[^/]+\\.md$"))))
              | .[].filename' "$files" > "$added"

    if grep -Eq '^changelog\.d/0+-' "$added"; then
        echo "::error::A changelog.d fragment still carries the 0000- placeholder. Rename it to changelog.d/${pr}-<slug>.md (see changelog.d/README.md)."
        return 1
    fi

    if [ ! -s "$added" ]; then
        echo "::error::This PR changes crates/ but adds no changelog.d/ fragment. Add changelog.d/${pr}-<slug>.md (see changelog.d/README.md) or apply the 'skip-changelog' label."
        return 1
    fi

    if ! grep -Eq "^changelog\.d/0*${pr}-" "$added"; then
        echo "::warning::No added changelog.d fragment is numbered ${pr}. That is expected for a backfill or a stacked PR; otherwise rename it, because a wrong number is invisible until a release is cut."
    fi
    return 0
}

# Exercises `changeset_verdict` itself, not a copy of it, so the check and its
# test cannot drift apart.
self_test() {
    local dir rc fails=0 out
    dir="$(mktemp -d)"
    trap 'rm -rf "$dir"' RETURN

    _case() { # name expected_rc expected_grep json [must_not_contain]
        local name="$1" want_rc="$2" want="$3" json="$4" reject="${5:-}"
        printf '%s\n' "$json" > "$dir/files.json"
        set +e
        out="$(changeset_verdict "$dir/files.json" 9010 2>&1)"
        rc=$?
        set -e
        if [ "$rc" != "$want_rc" ]; then
            echo "  FAIL  $name: rc=$rc want=$want_rc"; echo "$out" | sed 's/^/        /'
            fails=$((fails + 1)); return
        fi
        if [ -n "$want" ] && ! printf '%s' "$out" | grep -q "$want"; then
            echo "  FAIL  $name: output missing '$want'"; echo "$out" | sed 's/^/        /'
            fails=$((fails + 1)); return
        fi
        if [ -z "$want" ] && printf '%s' "$out" | grep -q '::warning::'; then
            echo "  FAIL  $name: unexpected warning"; echo "$out" | sed 's/^/        /'
            fails=$((fails + 1)); return
        fi
        if [ -n "$reject" ] && printf '%s' "$out" | grep -q "$reject"; then
            echo "  FAIL  $name: output should not contain '$reject'"; echo "$out" | sed 's/^/        /'
            fails=$((fails + 1)); return
        fi
        echo "  ok    $name"
    }

    _case "no crates/ change is not applicable" 0 "not applicable" \
        '[{"filename":"docs/src/x.md","status":"modified"}]'
    _case "crates/ change with the right number passes silently" 0 "" \
        '[{"filename":"crates/a/src/l.rs","status":"modified"},{"filename":"changelog.d/9010-x.md","status":"added"}]'
    _case "crates/ change with no fragment fails" 1 "adds no changelog.d" \
        '[{"filename":"crates/a/src/l.rs","status":"modified"}]'
    _case "0000- placeholder is a hard failure" 1 "0000- placeholder" \
        '[{"filename":"crates/a/src/l.rs","status":"modified"},{"filename":"changelog.d/0000-x.md","status":"added"}]'
    _case "0000- fails even alongside a correct fragment" 1 "0000- placeholder" \
        '[{"filename":"crates/a/src/l.rs","status":"modified"},{"filename":"changelog.d/0000-x.md","status":"added"},{"filename":"changelog.d/9010-y.md","status":"added"}]'
    _case "a backfill number warns but passes" 0 "::warning::" \
        '[{"filename":"crates/a/src/l.rs","status":"modified"},{"filename":"changelog.d/8944-x.md","status":"added"}]'
    _case "an EDITED fragment does not satisfy the gate" 1 "adds no changelog.d" \
        '[{"filename":"crates/a/src/l.rs","status":"modified"},{"filename":"changelog.d/9010-x.md","status":"modified"}]'
    _case "a nested path does not satisfy the gate" 1 "adds no changelog.d" \
        '[{"filename":"crates/a/src/l.rs","status":"modified"},{"filename":"changelog.d/sub/9010-x.md","status":"added"}]'
    # --paginate emits one array per page; the slurp must see every page, not
    # just the last (the bug the `-s` + `[.[][]]` form was written for).
    _case "a fragment on a later page still counts" 0 "" \
        '[{"filename":"crates/a/src/l.rs","status":"modified"}]
[{"filename":"changelog.d/9010-x.md","status":"added"}]' \
        "not applicable"
    # Mirror image: crates/ on the LAST page. Without the slurp, `jq -e`
    # reports only the last array and this one would pass by accident.
    _case "crates/ on a later page is still gated" 1 "adds no changelog.d" \
        '[{"filename":"docs/src/x.md","status":"modified"}]
[{"filename":"crates/a/src/l.rs","status":"modified"}]'

    if [ "$fails" -ne 0 ]; then
        echo "check_changeset_fragment: $fails self-test case(s) FAILED"
        return 1
    fi
    echo "check_changeset_fragment: self-test passed"
    return 0
}

case "${1:-}" in
    --self-test) self_test ;;
    -h|--help|"") usage; exit 2 ;;
    *)
        if [ "$#" -ne 2 ]; then usage; exit 2; fi
        tmp="$(mktemp -d)"
        trap 'rm -rf "$tmp"' EXIT
        gh api "repos/$1/pulls/$2/files" --paginate > "$tmp/files.json"
        changeset_verdict "$tmp/files.json" "$2"
        ;;
esac
