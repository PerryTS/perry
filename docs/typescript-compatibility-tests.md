# TypeScript Compatibility Test Suite

A comprehensive edge-case test suite for the Perry TypeScript compiler, designed to surface bugs and compatibility gaps by comparing Perry's native output against Node.js running the same TypeScript source.

> **⚠️ The status figure below is historical (v0.4.50).** Perry's compatibility
> has moved far past this snapshot: as of v0.5.x it sits at roughly **95%
> Node.js/TypeScript parity** and passes **~97%** of Node's own test suite
> (node v26). Day-to-day parity is now tracked by the gap test suite
> (`test-files/test_gap_*.ts`, byte-for-byte vs `node --experimental-strip-types`)
> and the Node module suite (`scripts/node_suite_run.py` against
> `test-parity/node_suite_baseline.json`). See
> [`runtime-parity-gaps.md`](runtime-parity-gaps.md) for the maintained gap list.

**Status (v0.4.50):** 7 of 26 tests pass with full parity against Node.js. Several others are within 1–6 output-line diffs of passing.

---

## What Is This?

The goal of this suite is to find the kinds of bugs we've historically only discovered in production:

- NaN-boxing edge cases (boolean tags, null vs undefined, negative numbers)
- Closure capture of module-level / block-scoped variables
- Class inheritance with `super.method()` in subclass methods
- Type coercion at string/number/boolean boundaries
- Union types and discriminated dispatch
- Array/Object/Map/Set method return types
- Destructuring (nested, defaults, rest)
- Control flow (labeled breaks, switch fallthrough, try/catch/finally)
- Promise chains, async/await tuple returns
- Generic classes and methods
- Math / Number static methods and constants
- String methods (trimStart/End, replaceAll, matchAll)
- Optional chaining side effects

Each test is a self-contained `.ts` file that asserts correctness via `console.log` output. The suite runs every test with both Node.js (`--experimental-strip-types`) and Perry's native compiler, then diffs the output line-by-line.

---

## Where the Tests Live

All 26 edge-case tests live in `test-files/`:

```
test-files/
  test_edge_arrays.ts
  test_edge_class_advanced.ts
  test_edge_classes.ts
  test_edge_closures.ts
  test_edge_complex_patterns.ts
  test_edge_control_flow.ts
  test_edge_destructuring.ts
  test_edge_enums_const.ts
  test_edge_error_handling.ts
  test_edge_generics.ts
  test_edge_higher_order.ts
  test_edge_interfaces.ts
  test_edge_iteration.ts
  test_edge_json_regex.ts
  test_edge_map_set.ts
  test_edge_numeric.ts
  test_edge_objects_records.ts
  test_edge_operators.ts
  test_edge_promises.ts
  test_edge_regression.ts
  test_edge_rest_spread_defaults.ts
  test_edge_scope_hoisting.ts
  test_edge_strings.ts
  test_edge_truthiness.ts
  test_edge_type_coercion.ts
  test_edge_type_narrowing.ts
  multi-edge/
    helpers.ts
    index.ts
```

Each file focuses on one category (closures, classes, numeric, etc.) and ranges from ~30 to ~180 assertions. Total: **~1,357 assertions**.

> **Note:** When first added, these files were unintentionally excluded by a `test-*` pattern in `.gitignore` that accidentally matched the `test-files/` directory. That pattern has been fixed — the tests are now trackable and cross-platform.

---

## How the Tests Work

Tests use `console.log` output rather than explicit assertions because that matches the existing parity-testing infrastructure (`run_parity_tests.sh`). The expected value is shown in a trailing comment:

```typescript
const acc = makeAccumulator();
console.log(acc(5));   // 5
console.log(acc(3));   // 8
console.log(acc(10));  // 18
```

For each test file, the runner:

1. Runs `node --experimental-strip-types <file>` → captures stdout
2. Compiles with `perry <file> -o <bin>`, then runs `<bin>` → captures stdout
3. Normalizes whitespace / line endings
4. Compares line-by-line; any diff is a FAIL

A test **passes** only if the normalized outputs match exactly.

---

## Running the Tests

### Prerequisites

- Rust toolchain (for building Perry)
- Node.js ≥ 22 (needs `--experimental-strip-types`)
- Platform: macOS / Linux / Windows (all supported by Perry)

### One-time setup

```bash
# Build Perry compiler + runtime + stdlib
cargo build --release -p perry-runtime -p perry-stdlib
cargo build --release
```

### Run the full suite

The script below compiles each test with Perry, runs both Node and Perry outputs, normalizes, diffs, and prints per-test results plus a summary.

Save as `run_edge_parity.sh` in the project root:

```bash
#!/bin/bash

PERRY_BIN="./target/release/perry"
TEST_DIR="test-files"
OUT_DIR="${TMPDIR:-/tmp}/perry_edge_tests"
mkdir -p "$OUT_DIR"

PASS=0
FAIL=0
COMPILE_FAIL=0
CRASH=0
RESULTS=""

normalize() {
    sed 's/\r$//' | sed 's/[[:space:]]*$//' | sed '/^$/d'
}

for f in "$TEST_DIR"/test_edge_*.ts; do
    name=$(basename "$f" .ts)

    node_out=$(node --experimental-strip-types "$f" 2>/dev/null | normalize) || true

    compile_out=$($PERRY_BIN "$f" -o "$OUT_DIR/$name" 2>&1)
    if [ $? -ne 0 ]; then
        echo "COMPILE_FAIL: $name"
        echo "$compile_out" | tail -3 | sed 's/^/    /'
        COMPILE_FAIL=$((COMPILE_FAIL + 1))
        RESULTS="${RESULTS}COMPILE_FAIL: $name\n"
        continue
    fi

    perry_out=$("$OUT_DIR/$name" 2>&1 | normalize)
    run_rc=$?
    if [ $run_rc -gt 128 ]; then
        sig=$((run_rc - 128))
        echo "CRASH: $name (signal $sig)"
        CRASH=$((CRASH + 1))
        RESULTS="${RESULTS}CRASH: $name (signal $sig)\n"
        continue
    fi

    if [ "$node_out" = "$perry_out" ]; then
        echo "PASS: $name"
        PASS=$((PASS + 1))
        RESULTS="${RESULTS}PASS: $name\n"
    else
        node_lines=$(echo "$node_out" | wc -l | tr -d ' ')
        diff_lines=$(diff <(echo "$node_out") <(echo "$perry_out") | grep -c '^[<>]' || true)
        echo "FAIL: $name  ($diff_lines diff lines / $node_lines total)"
        diff <(echo "$node_out") <(echo "$perry_out") | head -12 | sed 's/^/    /'
        echo
        FAIL=$((FAIL + 1))
        RESULTS="${RESULTS}FAIL: $name ($diff_lines/$node_lines)\n"
    fi
done

echo
echo "========================================"
echo "EDGE-CASE TEST RESULTS"
echo "========================================"
echo "  PASS:         $PASS"
echo "  FAIL:         $FAIL"
echo "  COMPILE_FAIL: $COMPILE_FAIL"
echo "  CRASH:        $CRASH"
echo "  TOTAL:        $((PASS + FAIL + COMPILE_FAIL + CRASH))"
echo "========================================"
echo
echo "--- Per-test ---"
echo -e "$RESULTS"
```

Then:

```bash
chmod +x run_edge_parity.sh
./run_edge_parity.sh
```

### Run a single test

```bash
# Compile and run with Perry
./target/release/perry test-files/test_edge_closures.ts -o /tmp/test_edge_closures
/tmp/test_edge_closures

# Compare against Node.js
node --experimental-strip-types test-files/test_edge_closures.ts
```

### Cross-platform notes

- **Linux / macOS:** the shell script above works as-is.
- **Windows:** the script works in Git Bash or WSL. For native PowerShell, replace the shell loop with `Get-ChildItem test-files\test_edge_*.ts | ForEach-Object { ... }`.
- The `.ts` source files are platform-agnostic — no path separators, no OS-specific APIs, no filesystem calls.

---

## Current Results (v0.4.50)

| Status | Count |
|--------|-------|
| **PASS** | 7 |
| **FAIL** | 19 |
| **COMPILE_FAIL** | 0 |
| **CRASH** | 0 |

### Passing (100% parity with Node)

1. `test_edge_error_handling` — try/catch/finally, error types, nested exceptions, error propagation
2. `test_edge_interfaces` — polymorphic dispatch, generic interfaces, structural typing, interface arrays
3. `test_edge_operators` — arithmetic, bitwise, comparison, logical, nullish coalescing, typeof, optional chaining
4. `test_edge_promises` — async/await, Promise.all, chaining, nested async functions
5. `test_edge_regression` — 18 historical regression cases from v0.4.x bugs
6. `test_edge_truthiness` — NaN-box booleans, `!!` coercion, `Boolean()`, all falsy values
7. `test_edge_type_narrowing` — typeof/instanceof guards, `in` operator, discriminated unions

### Close to passing (≤ 6 diff lines)

| Test | Diff lines | What's left |
|------|-----------:|-------------|
| `test_edge_numeric` | 2 | `Math.round(-0.5)` prints `0` instead of `-0` (cosmetic) |
| `test_edge_higher_order` | 2 | Memoization closure capturing a `Map` |
| `test_edge_strings` | 4 | `lastIndexOf` unimplemented; `"str " + array` concatenation calls default Object.toString |
| `test_edge_objects_records` | 6 | Object spread override `{ ...base, x: 10 }` doesn't override |
| `test_edge_rest_spread_defaults` | 6 | Same spread override bug; `[..."hello"]` spreads a string into garbage |
| `test_edge_control_flow` | 6 | Labeled `break outer` / `continue outer` not supported; `do...while` not supported |

### Moderate failures (7–20 diff lines)

`test_edge_closures`, `test_edge_class_advanced`, `test_edge_classes`, `test_edge_iteration`, `test_edge_map_set`, `test_edge_generics`, `test_edge_type_coercion`, `test_edge_json_regex`, `test_edge_enums_const`, `test_edge_arrays`, `test_edge_scope_hoisting`

### Major failures (> 20 diff lines)

`test_edge_complex_patterns` — uses `Record<string, Record<string, string>>` (nested records), which Perry doesn't fully support
`test_edge_destructuring` — nested patterns, rest, defaults, computed keys all have gaps
`test_edge_control_flow` — labeled break/continue and do-while cascade into failures

---

## Bugs Found and Fixed This Session (v0.4.50)

### 1. Boolean return values returned as f64 `1.0`/`0.0` instead of NaN-boxed tags

**Impact:** Many tests, including all uses of `.has()`, `.includes()`, `.startsWith()`, `.endsWith()`, `.test()`, `isNaN`, `isFinite`, `instanceof`.

**Root cause:** Codegen for these methods used `fcvt_from_sint(F64, i32)` to turn a 0/1 return into f64. The result was a plain number `0.0` or `1.0`, not a NaN-boxed boolean. `console.log` correctly prints numbers as `0`/`1`, so tests expecting `true`/`false` failed.

**Fix:**
- Added `i32_to_nanbox_bool(builder, val_i32)` helper in `crates/perry-codegen/src/util.rs` that emits `select(val_i32 != 0, TAG_TRUE, TAG_FALSE)` and returns an f64.
- Replaced every buggy call site in `crates/perry-codegen/src/expr.rs` (Map.has/delete, Set.has/delete, Array.includes, String.startsWith/endsWith/includes, inline method-dispatch paths).
- Runtime functions `js_is_nan`, `js_is_finite`, `js_instanceof` updated to return NaN-boxed booleans directly.

### 2. `super.method()` in subclass methods caused compile error

**Impact:** Any TypeScript program with subclass method overrides that call `super.X()`. Error: `super.X() called outside of class context`.

**Root cause:** The method inlining pass in `crates/perry-transform/src/inline.rs` inlines small methods into their call sites. When the subclass method contained `Expr::SuperCall` or `Expr::SuperMethodCall`, the inlined body ended up in the caller (e.g., `main()`) where there is no `ThisContext`, so compilation of the super call failed.

**Fix:** Added `body_contains_super_call()` in `inline.rs` and made `is_inlinable()` reject any method whose body contains a super call. These methods are now always compiled as real methods, preserving the class context.

### 3. `Number.MAX_SAFE_INTEGER` and `Number.isNaN/isFinite/isInteger/isSafeInteger` unsupported

**Impact:** Any code using these constants or the strict Number predicates returned `undefined`.

**Fix:**
- Added constant handling in `crates/perry-hir/src/lower.rs` for the entire `Number` namespace (`MAX_SAFE_INTEGER`, `MIN_SAFE_INTEGER`, `EPSILON`, `MAX_VALUE`, `MIN_VALUE`, `POSITIVE_INFINITY`, `NEGATIVE_INFINITY`, `NaN`).
- Added HIR variants `NumberIsNaN`, `NumberIsFinite`, `NumberIsInteger`, `NumberIsSafeInteger`.
- Added runtime functions `js_number_is_nan/is_finite/is_integer/is_safe_integer` in `crates/perry-runtime/src/builtins.rs` that properly distinguish plain numbers from NaN-boxed tag values.

### 4. `Math.trunc` and `Math.sign` missing

**Fix:** Desugared at HIR level in `lower.rs`:
- `Math.trunc(x)` → `x >= 0 ? floor(x) : ceil(x)`
- `Math.sign(x)` → `x > 0 ? 1 : x < 0 ? -1 : x`

### 5. `Math.round(0.5)` returned 0

**Root cause:** The `nearest` (roundeven) instruction uses IEEE 754 round-half-to-even (banker's rounding). `0.5` rounds to `0` because `0` is even. JS uses round-half-away-from-zero for positives (`0.5` → `1`).

**Fix:** `MathRound` now emits `floor(x + 0.5)` in `expr.rs`.

### 6. `!null`, `!undefined`, `!NaN`, `!!("" + "")` all wrong

**Impact:** Many boolean expressions involving null/undefined/NaN or string concatenation.

**Root cause:** The unary Not handler had a `needs_truthy_check` list that decided between calling `js_is_truthy` and doing a naive `fcmp(val, 0.0)`. The list was missing `Null`, `Undefined`, `Array`, `Object`, `New`, `BooleanCoerce`, `NumberIsX`, string-producing `Binary::Add`, `Logical`, `Conditional`. For plain numeric values, the fallback `(val == 0)` was wrong because NaN is also falsy in JS.

**Fix:**
- Expanded `needs_truthy_check` to a recursive helper `expr_yields_nanboxed()` that handles all NaN-boxed operand kinds including binary add of strings, logical/conditional returning strings, etc.
- Numeric fallback now uses `(val == 0) || (val != val)` to treat NaN as falsy.

### 7. `"" || "default"` returned `""`

**Root cause:** `const s = ""; s || "default"` — the string variable is stored as I64 (raw pointer) because `is_string && is_pointer`. The Logical Or fast path checked `(ptr != 0)` which is true for any allocated empty string, so JS's "empty string is falsy" semantic was lost.

**Fix:** `LogicalOp::Or` now detects string operands and NaN-boxes the I64 pointer (via `inline_nanbox_string`) before calling `js_is_truthy`.

### 8. `null === undefined` returned `true`

**Root cause:** The `Compare` codegen had a "null compare" fast path that OR'd together all three nullish representations (`TAG_NULL`, `TAG_UNDEFINED`, raw null). That's JS **loose** equality semantics (`==`). TypeScript only has strict `===`.

**Fix:** For strict equality, Perry now compares the value against the *specific* tag of the literal: `null === x` checks for `TAG_NULL` only; `undefined === x` checks for `TAG_UNDEFINED` only. So `null === undefined` correctly returns `false`.

### 9. `Infinity` printed as `inf`, `NaN` as `NaN` sometimes, `-0` as `0`

**Root cause:** `js_number_to_string`, `js_string_coerce`, and `js_array_join` used Rust's `format!("{}", f)` which produces `inf`/`-inf`. JS spec requires `Infinity`/`-Infinity`.

**Fix:** All three runtime functions now branch on `is_nan()` / `is_infinite()` first and emit JS-correct strings.

### 10. Class named `EventEmitter` collided with Perry's native EventEmitter

**Root cause:** When user code declares `class EventEmitter { ... }` with its own `on`/`emit` methods, Perry's HIR lowering confuses calls to those methods with Perry's built-in EventEmitter (from the `events` module), which has a different signature. The result is a codegen error: `mismatched argument count: got 1, expected 3`.

**Workaround:** The test file renames the class to `MyEmitter`.

**Proper fix (TODO):** User classes should take precedence over built-in native classes when the user explicitly declares one. This is in the HIR `lookup_class`/`native_extends` resolution logic.

---

## Before / After Summary

| | Before | After |
|---|---|---|
| Passing | 3 | **7** |
| Failing | 21 | 19 |
| Compile failures | 2 | **0** |
| Runtime crashes | 0 | 0 |

Net: **+4 tests passing**, no regressions, two previously-broken features (`super.method()` and `Number.*`) now work.

---

## What to Do Next

The suite is set up so anyone can run it on their machine and see exactly where Perry deviates from Node.js. Here are the remaining high-value fixes, ordered by estimated ROI:

### Quick wins (1–2 diff lines, each unlocks a full test)

1. **`Math.round(-0.5)` prints `0` instead of `-0`.** Cosmetic but counts as a diff. Either special-case `-0` in `js_console_log_dynamic`, or emit `-0.0` from the round codegen when input is in `(-0.5, 0]`.
2. **Memoize closure capturing a `Map`.** Test: `test_edge_higher_order`. The closure body's `cache.has(n)` always returns false, so every call is a cache miss. Likely root cause: the closure captures `cache` as a value type rather than a pointer, so a fresh Map is created each invocation.

### Medium wins (4–6 diff lines)

3. **`String.prototype.lastIndexOf`** — not implemented. Add `js_string_last_index_of` in `perry-runtime/src/string.rs` mirroring `js_string_index_of` but scanning from the end.
4. **`"prefix" + array` concatenation** — currently produces `prefix [object Object]`. JS calls `Array.prototype.toString()` which is equivalent to `.join(",")`. In the string concatenation codegen (`expr.rs`, Binary::Add path with string on one side), detect when the other operand is an array and emit a call to `js_array_join` with `","`.
5. **Object spread override `{...base, x: 10}`** — the later key should win. Currently Perry keeps the original. Fix is in `expr.rs` `Expr::ObjectSpread`: iterate spread keys first, then explicit keys, letting explicit keys overwrite.
6. **`[..."hello"]`** — spreading a string into an array should produce `["h","e","l","l","o"]`. Currently produces garbage. Add a special case in array-spread codegen for string operands that iterates the string and pushes each char.

### Medium-large fixes (7–20 diff lines, affect multiple tests)

7. **Block scoping for `let` / `const`.** Affects `test_edge_scope_hoisting` and `test_edge_enums_const`. Inner `let` shadowing outer `let` leaks — the inner scope's value escapes. Requires tracking scope depth in HIR lowering and creating fresh `LocalId`s for inner scopes.
8. **Labeled `break` / `continue`.** Affects `test_edge_control_flow`. Requires:
   - Adding `Stmt::Label(String, Box<Stmt>)`, `Stmt::LabeledBreak(String)`, `Stmt::LabeledContinue(String)` in `crates/perry-hir/src/ir.rs`.
   - HIR lowering support in `lower.rs` for `ast::LabeledStmt`, `ast::BreakStmt { label: Some(_) }`, `ast::ContinueStmt { label: Some(_) }`.
   - Codegen support in `stmt.rs`: `LoopContext` needs a stack of `(label, break_block, continue_block)` instead of just one entry.
9. **`do...while`.** Add `Stmt::DoWhile { body, condition }`. The only difference from `while` is the body runs once unconditionally before checking the condition. Straightforward codegen.
10. **Nested destructuring with defaults and rest.** Affects `test_edge_destructuring`. The pattern lowering in `lower.rs` handles top-level destructuring OK but nested `{ outer: { inner } }` and `[a = 1, ...rest]` need work.
11. **Arrow fn expression body (`() => value`) inside object literals with multiple closures.** Affects `test_edge_closures`. Works for single-closure objects, fails for multi-closure. Likely related to closure capture deduplication or object-literal property value compilation order.

### Large fixes (> 20 diff lines)

12. **Nested `Record<string, Record<string, T>>`.** Affects `test_edge_complex_patterns`. State machine, observer, pipeline patterns all use this.
13. **`for...of` on strings.** Currently produces corrupted output. Requires iterating character-by-character instead of treating the string as an array.
14. **User class named the same as a built-in (e.g., `EventEmitter`).** Resolve user declarations before built-ins in `HIR lookup_class`.

### Testing hygiene

15. **Un-ignore `test-files/` in `.gitignore`.** ✅ Done in v0.4.50 — previously the `test-*` pattern matched the directory, so the test sources were silently untracked.
16. **Add this script to CI.** The `run_edge_parity.sh` script above could run on every PR. Threshold: pass count must not decrease.
17. **Add a `cargo xtask edge-parity` task** so it's easy to run from any directory and on any OS without shell differences.
18. **Split `test_edge_arrays.ts` / `test_edge_classes.ts`** into smaller files — a single bug can cascade and skew the diff count. Smaller files give more precise signal.

---

## Where to Start If You're Joining the Effort

1. Run the suite: `./run_edge_parity.sh`
2. Pick a failing test with ≤ 6 diff lines from the "Close to passing" table.
3. Read just the failing assertions: `diff <(node --experimental-strip-types test-files/<test>.ts) <(/tmp/perry_edge_tests/<test>)`.
4. Reduce to the minimal failing snippet in `/tmp/test_repro.ts`.
5. Find the relevant codegen path:
   - Method dispatch: `crates/perry-codegen/src/expr.rs` — search for method name
   - Runtime function: `crates/perry-runtime/src/*.rs`
   - HIR lowering: `crates/perry-hir/src/lower.rs`
   - Method inlining: `crates/perry-transform/src/inline.rs`
6. Fix, rebuild:
   ```bash
   cargo build --release -p perry-runtime -p perry-stdlib
   cargo build --release
   ```
7. Re-run the suite to verify no regressions.
8. Follow `CLAUDE.md` workflow: bump patch version, add Recent Changes entry, commit.

---

## Files Modified in v0.4.50 (this session)

**New test files (26 in `test-files/` plus `test-files/multi-edge/`).**

**Codegen:**
- `crates/perry-codegen/src/util.rs` — `i32_to_nanbox_bool` helper
- `crates/perry-codegen/src/expr.rs` — boolean returns, Unary Not, Logical Or with strings, strict null equality, Math.round, `NumberIs*`
- `crates/perry-codegen/src/closures.rs` — walker coverage for new HIR variants
- `crates/perry-codegen/src/runtime_decls.rs` — declared `js_number_is_*`
- `crates/perry-codegen-js/src/emit.rs` — JS backend emit for `NumberIs*`

**HIR:**
- `crates/perry-hir/src/ir.rs` — `NumberIsNaN/Finite/Integer/SafeInteger` variants
- `crates/perry-hir/src/lower.rs` — `Number.*` constants and methods, `Math.trunc`/`sign`

**Transform:**
- `crates/perry-transform/src/inline.rs` — `body_contains_super_call()` check

**Runtime:**
- `crates/perry-runtime/src/builtins.rs` — `js_is_nan`/`js_is_finite`/`js_string_coerce` Infinity formatting; new `js_number_is_*` functions
- `crates/perry-runtime/src/object.rs` — `js_instanceof` returns NaN-boxed bool
- `crates/perry-runtime/src/string.rs` — `js_number_to_string` Infinity/NaN/−0 formatting
- `crates/perry-runtime/src/array.rs` — `js_array_join` Infinity/NaN formatting

**Misc:**
- `CLAUDE.md` — version bump, Recent Changes entry
- `Cargo.toml` — `0.4.49` → `0.4.50`
- `.gitignore` — fixed `test-*` pattern that was excluding `test-files/`
- `test-files/test_edge_classes.ts` — renamed `EventEmitter` → `MyEmitter` (works around a known class-name collision bug)
