#!/bin/bash
# Regression for #945: a non-escaping `new C()` followed by a trivial
# `obj.method()` where `method() { return this.field; }` should scalar-replace
# the instance instead of heap-allocating and dispatching the method.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [ -n "${PERRY_BIN:-}" ]; then
  PERRY="$PERRY_BIN"
  if [ ! -x "$PERRY" ]; then
    echo "FAIL: PERRY_BIN is not executable: $PERRY"
    exit 2
  fi
else
  PERRY="$REPO_ROOT/target/release/perry"
  [ ! -x "$PERRY" ] && PERRY="$REPO_ROOT/target/debug/perry"
  if [ ! -x "$PERRY" ]; then
    echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
    exit 0
  fi
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

cat > "$TMPDIR/main.ts" << 'EOF'
class MyClass {
  private value: number;
  constructor(value: number) {
    this.value = value;
  }
  getValue(): number {
    return this.value;
  }
}

function main(): number {
  const iterations = 1000;
  let start = performance.now();
  let sum = 0;
  for (let i = 0; i < iterations; i++) {
    const obj = new MyClass(1);
    sum += obj.getValue();
  }
  let end = performance.now();
  console.log(`${end - start} ms, sum: ${sum}`);
  return sum;
}

console.log("sum:" + main());
EOF

COMPILE_LOG="$TMPDIR/compile.log"
set +e
(
  cd "$TMPDIR"
  PERRY_LLVM_BITCODE_LINK=1 "$PERRY" compile main.ts \
    --no-link \
    --keep-intermediates \
    --no-auto-optimize \
    --no-cache >"$COMPILE_LOG" 2>&1
)
COMPILE_STATUS=$?
set -e

IR_FILE="$TMPDIR/main_ts.ll"
if [ ! -f "$IR_FILE" ]; then
  # Some CI runners invoke Perry from a workspace where the displayed
  # "Wrote LLVM IR: <path>" location is relative to the original shell
  # cwd, not the temporary compile cwd. Trust the compiler's reported
  # path before failing the guard.
  while IFS= read -r logged_ir; do
    [ -z "$logged_ir" ] && continue
    if [ -f "$logged_ir" ]; then
      cp "$logged_ir" "$IR_FILE"
      break
    fi
    if [ -f "$TMPDIR/$logged_ir" ]; then
      cp "$TMPDIR/$logged_ir" "$IR_FILE"
      break
    fi
  done < <(sed -n 's/^Wrote LLVM IR: //p' "$COMPILE_LOG")
fi

if [ ! -f "$IR_FILE" ]; then
  echo "FAIL: expected LLVM IR file was not emitted"
  echo "Files under temp dir:"
  find "$TMPDIR" -maxdepth 2 -type f -print || true
  cat "$COMPILE_LOG"
  exit 1
fi

if [ "$COMPILE_STATUS" -ne 0 ] && ! grep -q "clang not found" "$COMPILE_LOG"; then
  echo "FAIL: compile failed before IR verification"
  cat "$COMPILE_LOG"
  exit 1
fi

MAIN_IR="$TMPDIR/main_fn.ll"
awk '/^define double @perry_fn_main_ts__main\(/,/^}/' "$IR_FILE" > "$MAIN_IR"

if [ ! -s "$MAIN_IR" ] || ! grep -q '^define double @perry_fn_main_ts__main(' "$MAIN_IR"; then
  echo "FAIL: expected main() LLVM function was not found in emitted IR"
  grep -En '^define .*@perry_fn_main_ts__main\(' "$IR_FILE" || true
  exit 1
fi

if grep -Eq 'call .*@(js_inline_arena_state|js_object_alloc|js_object_alloc_class|perry_method_.*MyClass.*getValue|js_native_call_method)' "$MAIN_IR"; then
  echo "FAIL: scalar field-return method still allocates or dispatches"
  grep -En 'call .*@(js_inline_arena_state|js_object_alloc|js_object_alloc_class|perry_method_.*MyClass.*getValue|js_native_call_method)' "$MAIN_IR" || true
  exit 1
fi

echo "PASS issue #945 scalar method IR guard"
