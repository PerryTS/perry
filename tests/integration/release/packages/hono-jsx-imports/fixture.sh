#!/usr/bin/env bash
# Tier-3 fixture: hono-jsx-imports (#1671).
#
# Verifies that the `hono/jsx` + `hono/jsx/streaming` module imports —
# compiled natively via `perry.compilePackages` — resolve and LINK.
# hono/jsx/streaming transitively imports hono/jsx/hooks, whose 1-arg
# `setTimeout(() => …)` re-render scheduler previously fell through
# codegen's extern-func table to a bare `@setTimeout` and failed at link
# with `Undefined symbols: _setTimeout`. The entry imports the runtime
# helpers and prints their typeof, byte-for-byte matching Node.
#
# Acceptance:
#   - `npm install` resolves hono into ./node_modules
#   - `perry entry.ts -o ./out` exits 0 (the regression: it failed at LINK)
#   - `./out` produces stdout matching expected.txt byte-for-byte
#   - `node --experimental-strip-types entry.ts` ALSO produces that same
#     output. The entry deliberately uses imports only (no JSX syntax) so
#     strip-types can run it — node does not transform JSX.

set -uo pipefail
cd "$(dirname "$0")"

NAME="hono-jsx-imports"
PERRY_BIN="${PERRY_BIN:-../../../target/release/perry}"

# 1. Resolve dependencies. node_modules is gitignored at the repo level
# so first run on a fresh checkout pulls hono. Subsequent runs are fast.
if [[ ! -d node_modules/hono ]]; then
    echo "  [1/4] npm install hono..."
    npm install --silent --no-audit --no-fund > install.log 2>&1
    if [[ ! -d node_modules/hono ]]; then
        echo "FAIL $NAME — npm install did not produce node_modules/hono"
        sed 's/^/    /' install.log
        exit 1
    fi
fi

# 2. Compile with Perry. This is where #1671 regressed (link failure).
echo "  [2/4] perry compile entry.ts..."
if ! "$PERRY_BIN" entry.ts -o ./out > perry-compile.log 2>&1; then
    echo "FAIL $NAME — perry compile errored"
    sed 's/^/    /' perry-compile.log | tail -40
    exit 1
fi

# 3. Run the compiled binary and capture stdout.
echo "  [3/4] ./out..."
if ! ./out > perry-out.txt 2> perry-run.log; then
    echo "FAIL $NAME — perry binary exited non-zero"
    sed 's/^/    /' perry-run.log | tail -40
    echo "    --- stdout (truncated) ---"
    sed 's/^/    /' perry-out.txt | tail -20
    exit 1
fi

# 4. Diff against expected.txt. Optionally re-run Node to verify
# expected.txt is still accurate against the pinned hono version.
echo "  [4/4] diff against expected.txt..."
if ! diff -u expected.txt perry-out.txt > diff.log; then
    echo "FAIL $NAME — perry output diverges from expected.txt"
    sed 's/^/    /' diff.log
    if command -v node >/dev/null 2>&1; then
        node --experimental-strip-types entry.ts > node-out.txt 2>/dev/null || true
        if [[ -s node-out.txt ]] && ! diff -q expected.txt node-out.txt > /dev/null 2>&1; then
            echo "    NOTE: expected.txt is also out of sync with node — refresh it"
            diff -u expected.txt node-out.txt | head -20 | sed 's/^/      /'
        fi
    fi
    exit 1
fi

echo "PASS $NAME"
