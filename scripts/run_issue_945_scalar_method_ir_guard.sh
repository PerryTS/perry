#!/bin/bash
# Regression for #945: a non-escaping `new C()` followed by a trivial
# `obj.method()` where `method() { return this.field; }` should scalar-replace
# the instance instead of heap-allocating and dispatching the method.
#
# The mirror half is just as important: when method lookup is NOT statically
# stable (own-property write, prototype mutation, …) the instance MUST be
# heap-allocated and the method MUST be dispatched (#5872).

set -e

# Every helper a `new C()` site may lower to. #5294 outlined the per-new-site
# inline bump allocator, so class instances now allocate through
# `js_object_alloc_class_inline_keys` instead of emitting an inline
# `js_inline_arena_state` sequence; array literals still use the inline form.
# Both spellings count as "this object was heap-allocated".
ALLOC_RE='call .*@(js_inline_arena_state|js_object_alloc)'
DISPATCH_RE='call .*@(js_native_call_value|js_native_call_method|perry_method_.*__getValue)'

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

cat > "$TMPDIR/unsafe.ts" << 'EOF'
class OwnMethodWrite {
  value = 14;
  getValue(): number {
    return this.value;
  }
}
function ownMethodWrite(): number {
  const obj = new OwnMethodWrite();
  (obj as any).getValue = () => 99;
  return obj.getValue();
}

class PrototypeDirectWriteMethod {
  value = 21;
  getValue(): number {
    return this.value;
  }
}
function prototypeDirectWrite(): number {
  const obj = new PrototypeDirectWriteMethod();
  (PrototypeDirectWriteMethod.prototype as any).getValue = function () {
    return 111;
  };
  return obj.getValue();
}

class PrototypeDefinePropertyMethod {
  value = 22;
  getValue(): number {
    return this.value;
  }
}
function prototypeDefineProperty(): number {
  const obj = new PrototypeDefinePropertyMethod();
  Object.defineProperty(PrototypeDefinePropertyMethod.prototype, "getValue", {
    value: function () {
      return 112;
    },
  });
  return obj.getValue();
}

class PrototypeComputedWriteMethod {
  value = 23;
  getValue(): number {
    return this.value;
  }
}
function prototypeComputedWrite(): number {
  const obj = new PrototypeComputedWriteMethod();
  const key = "getValue";
  (PrototypeComputedWriteMethod.prototype as any)[key] = function () {
    return 113;
  };
  return obj.getValue();
}

class PrototypeUnknownCallMethod {
  value = 24;
  getValue(): number {
    return this.value;
  }
}
function mutatePrototypeThroughUnknownCall(): void {
  (PrototypeUnknownCallMethod.prototype as any).getValue = function () {
    return 114;
  };
}
function prototypeUnknownCall(): number {
  const obj = new PrototypeUnknownCallMethod();
  mutatePrototypeThroughUnknownCall();
  return obj.getValue();
}

class ConstructorPrototypeCallMethod {
  value = 25;
  constructor() {
    mutateConstructorPrototypeMethod();
  }
  getValue(): number {
    return this.value;
  }
}
function mutateConstructorPrototypeMethod(): void {
  (ConstructorPrototypeCallMethod.prototype as any).getValue = function () {
    return 115;
  };
}
function constructorPrototypeCall(): number {
  const obj = new ConstructorPrototypeCallMethod();
  return obj.getValue();
}

class FieldInitializerPrototypeCallMethod {
  value = mutateFieldInitializerPrototypeMethod();
  getValue(): number {
    return this.value;
  }
}
function mutateFieldInitializerPrototypeMethod(): number {
  (FieldInitializerPrototypeCallMethod.prototype as any).getValue = function () {
    return 116;
  };
  return 0;
}
function fieldInitializerPrototypeCall(): number {
  const obj = new FieldInitializerPrototypeCallMethod();
  return obj.getValue();
}

class SetPrototypeMethod {
  value = 26;
  getValue(): number {
    return this.value;
  }
}
function setPrototypeMethod(): number {
  const obj = new SetPrototypeMethod();
  Object.setPrototypeOf(obj, {
    getValue() {
      return 117;
    },
  });
  return obj.getValue();
}

console.log(
  ownMethodWrite() +
    prototypeDirectWrite() +
    prototypeDefineProperty() +
    prototypeComputedWrite() +
    prototypeUnknownCall() +
    constructorPrototypeCall() +
    fieldInitializerPrototypeCall() +
    setPrototypeMethod(),
);
EOF

UNSAFE_COMPILE_LOG="$TMPDIR/unsafe_compile.log"
set +e
(
  cd "$TMPDIR"
  PERRY_LLVM_BITCODE_LINK=1 "$PERRY" compile unsafe.ts \
    --no-link \
    --keep-intermediates \
    --no-auto-optimize \
    --no-cache >"$UNSAFE_COMPILE_LOG" 2>&1
)
UNSAFE_COMPILE_STATUS=$?
set -e

UNSAFE_IR_FILE="$TMPDIR/unsafe_ts.ll"
if [ ! -f "$UNSAFE_IR_FILE" ]; then
  while IFS= read -r logged_ir; do
    [ -z "$logged_ir" ] && continue
    if [ -f "$logged_ir" ]; then
      cp "$logged_ir" "$UNSAFE_IR_FILE"
      break
    fi
    if [ -f "$TMPDIR/$logged_ir" ]; then
      cp "$TMPDIR/$logged_ir" "$UNSAFE_IR_FILE"
      break
    fi
  done < <(sed -n 's/^Wrote LLVM IR: //p' "$UNSAFE_COMPILE_LOG")
fi

if [ ! -f "$UNSAFE_IR_FILE" ]; then
  echo "FAIL: expected unsafe LLVM IR file was not emitted"
  cat "$UNSAFE_COMPILE_LOG"
  exit 1
fi

if [ "$UNSAFE_COMPILE_STATUS" -ne 0 ] && ! grep -q "clang not found" "$UNSAFE_COMPILE_LOG"; then
  echo "FAIL: unsafe compile failed before IR verification"
  cat "$UNSAFE_COMPILE_LOG"
  exit 1
fi

for fn in \
  ownMethodWrite \
  prototypeDirectWrite \
  prototypeDefineProperty \
  prototypeComputedWrite \
  prototypeUnknownCall \
  constructorPrototypeCall \
  fieldInitializerPrototypeCall \
  setPrototypeMethod; do
  FN_IR="$TMPDIR/${fn}.ll"
  awk "/^define double @perry_fn_unsafe_ts__${fn}\\(/,/^}/" "$UNSAFE_IR_FILE" > "$FN_IR"
  if [ ! -s "$FN_IR" ] || ! grep -q "^define double @perry_fn_unsafe_ts__${fn}(" "$FN_IR"; then
    echo "FAIL: expected unsafe ${fn}() LLVM function was not found"
    grep -En "^define .*@perry_fn_unsafe_ts__${fn}\\(" "$UNSAFE_IR_FILE" || true
    exit 1
  fi
  if ! grep -Eq "$ALLOC_RE" "$FN_IR"; then
    echo "FAIL: unsafe ${fn}() was scalarized instead of allocated"
    exit 1
  fi
  if ! grep -Eq "$DISPATCH_RE" "$FN_IR"; then
    echo "FAIL: unsafe ${fn}() lost method dispatch"
    exit 1
  fi
done

echo "PASS issue #945 scalar method IR guard"
