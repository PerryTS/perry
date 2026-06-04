#!/bin/bash
# Regression for #4374: generator .return()/.throw() must run pending
# non-yielding finally blocks when suspended inside a try.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY:-$REPO_ROOT/target/release/perry}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

COMPILE_ENV=(env PERRY_ALLOW_UNIMPLEMENTED=1)
if [[ -f "$REPO_ROOT/target/debug/libperry_runtime.a" || -f "$REPO_ROOT/target/release/libperry_runtime.a" ]]; then
    COMPILE_ENV=(env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE=1)
fi

cat > "$TMPDIR/main.ts" << 'EOF'
function show(label: string, value: any) {
  console.log(label + ":" + JSON.stringify(value));
}

function* returnsThroughFinally() {
  try {
    yield 1;
    yield 2;
  } finally {
    console.log("return-finally");
  }
}

const r = returnsThroughFinally();
show("return-next", r.next());
show("return-result", r.return(99));
show("return-after", r.next());

function* throwsThroughFinally() {
  try {
    yield "a";
  } finally {
    console.log("throw-finally");
  }
}

const t = throwsThroughFinally();
show("throw-next", t.next());
try {
  t.throw("boom");
} catch (e: any) {
  console.log("throw-caught:" + e);
}
show("throw-after", t.next());

function* catchThenFinally() {
  try {
    yield "x";
  } catch (e: any) {
    console.log("catch:" + e);
  } finally {
    console.log("catch-finally");
  }
  yield "after";
}

const c = catchThenFinally();
show("catch-next", c.next());
show("catch-throw", c.throw("caught"));
show("catch-after", c.next());

function* finalReturnOnReturn() {
  try {
    yield "fr";
  } finally {
    console.log("final-return-finally");
    return "override";
  }
}

const fr = finalReturnOnReturn();
show("final-return-next", fr.next());
show("final-return-result", fr.return(99));

function* finalReturnOnThrow() {
  try {
    yield "ft";
  } finally {
    console.log("final-throw-finally");
    return "override";
  }
}

const ft = finalReturnOnThrow();
show("final-throw-next", ft.next());
show("final-throw-result", ft.throw("ignored"));

function* nestedFinally() {
  try {
    try {
      yield "n";
    } finally {
      console.log("nested-inner");
    }
  } finally {
    console.log("nested-outer");
  }
}

const n = nestedFinally();
show("nested-next", n.next());
show("nested-return", n.return("done"));
EOF

"${COMPILE_ENV[@]}" "$PERRY" compile --no-cache "$TMPDIR/main.ts" -o "$TMPDIR/test_bin" \
    >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

RUN_OUTPUT="$("$TMPDIR/test_bin" 2>&1)"

EXPECTED='return-next:{"value":1,"done":false}
return-finally
return-result:{"value":99,"done":true}
return-after:{"done":true}
throw-next:{"value":"a","done":false}
throw-finally
throw-caught:boom
throw-after:{"done":true}
catch-next:{"value":"x","done":false}
catch:caught
catch-finally
catch-throw:{"value":"after","done":false}
catch-after:{"done":true}
final-return-next:{"value":"fr","done":false}
final-return-finally
final-return-result:{"value":"override","done":true}
final-throw-next:{"value":"ft","done":false}
final-throw-finally
final-throw-result:{"value":"override","done":true}
nested-next:{"value":"n","done":false}
nested-inner
nested-outer
nested-return:{"value":"done","done":true}'

if [[ "$RUN_OUTPUT" == "$EXPECTED" ]]; then
    echo "PASS"
    exit 0
fi

echo "FAIL: generator abrupt finally output changed"
echo "Expected:"
echo "$EXPECTED"
echo
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
