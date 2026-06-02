#!/bin/bash
# Regression coverage for #3583 / Test262 async-function semantic parity:
# direct async bodies must resolve/reject through Promises, default-param
# abrupt completions must reject, and `.length` must stop at the first default.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

COMPILE_ENV=()
if [ -f "$SCRIPT_DIR/../target/debug/libperry_runtime.a" ] || [ -f "$SCRIPT_DIR/../target/release/libperry_runtime.a" ]; then
  COMPILE_ENV=(env PERRY_NO_AUTO_OPTIMIZE=1)
fi

cat > "$TMPDIR/main.ts" << 'EOF'
async function returnsValue() {
  return 42;
}

function failDefault(): number {
  throw "default-boom";
}

function failArrowDefault(): number {
  throw "arrow-default-boom";
}

async function rejectsDefault(value = failDefault()) {
  console.log("default body ran");
  return value;
}

async function throwsNow() {
  throw "throw-boom";
}

async function lengthOne(a: number, b = 39,) {
  return a + b;
}

const arrowLengthOne = async (a: number, b = 39,) => {
  return a + b;
};

const rejectsArrowDefault = async (value = failArrowDefault()) => {
  console.log("arrow body ran");
  return value;
};

returnsValue().then((value) => console.log("return " + value));
rejectsDefault().then(
  () => console.log("default unexpectedly resolved"),
  (err) => console.log("default rejected " + err)
);
throwsNow().then(
  () => console.log("throw unexpectedly resolved"),
  (err) => console.log("throw rejected " + err)
);
rejectsArrowDefault().then(
  () => console.log("arrow default unexpectedly resolved"),
  (err) => console.log("arrow default rejected " + err)
);

console.log("fn length " + lengthOne.length);
console.log("arrow length " + arrowLengthOne.length);
console.log("sync end");
EOF

cd "$TMPDIR"
"${COMPILE_ENV[@]}" "$PERRY" compile main.ts --output test_bin --no-cache >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)

EXPECTED="fn length 1
arrow length 1
sync end
return 42
default rejected default-boom
throw rejected throw-boom
arrow default rejected arrow-default-boom"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: async semantic parity regression"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
