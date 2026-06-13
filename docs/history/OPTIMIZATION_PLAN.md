# Perry Optimization Plan - Next Steps

## Current State (Feb 2026)

### Perry Wins (no optimization needed)
| Benchmark | Perry | Node | Factor |
|-----------|-------|------|--------|
| sort | 1ms | 31ms | 31x faster |
| prime-sieve | 2ms | 6ms | 3x faster |
| array-ops | <1ms | 3ms | >3x faster |
| json-parse | <1ms | 0.5ms | parity |

### Gaps to Close
| Benchmark | Perry | Node | Gap | Priority |
|-----------|-------|------|-----|----------|
| string-ops | 1ms | 0.1ms | 10x slower | **P0** (runtime crash) |
| fibonacci | 1271ms | 1049ms | 1.2x slower | P1 |
| object-create | 12ms | 5ms | 2.4x slower | P2 |
| matrix-multiply | 43ms | 15ms | 2.8x slower | P2 |

---

## P0: Fix String Runtime Crashes

**Impact:** Unblocks 10x improvement in string-ops, enables idiomatic TypeScript patterns

### Issue 1: `indexOf` crashes Perry

**Reproduction:**
```typescript
const s = "ABCDEFABC";
const idx = s.indexOf("ABC");      // SIGSEGV
const idx2 = s.indexOf("ABC", 1);  // SIGSEGV
```

**Location:** `crates/perry-runtime/src/string.rs` — `js_string_index_of` and `js_string_index_of_from`

**Likely cause:** The runtime function exists (uses Rust's `str::find()`), but the codegen may be passing incorrect arguments or the NaN-boxing of the return value (-1 for not found) may be broken.

**Fix approach:**
1. Add a standalone test case in Perry's test suite
2. Check codegen for `indexOf` calls — verify argument marshaling
3. Check return value handling for -1 (should be valid f64, not a special NaN-boxed value)

### Issue 2: `split('')` crashes Perry

**Reproduction:**
```typescript
const s = "ABC";
const chars = s.split('');  // SIGSEGV
```

**Location:** `crates/perry-runtime/src/string.rs` — `js_string_split`

**Likely cause:** Empty string as delimiter is a special case (split into individual characters). The runtime may not handle this pattern.

**Fix approach:**
1. Check if `js_string_split` handles empty delimiter
2. If not, add special case: when delimiter is empty string, iterate UTF-8 chars and create array

---

## P1: Improve Fibonacci (1.2x slower)

**Current:** 1271ms Perry vs 1049ms Node (was 2778ms before optimizations)

The 2.2x improvement from compiler optimizations was significant, but there's still a 22% gap.

### Root cause analysis

The remaining overhead is likely from:
1. **Function call overhead** — Each recursive call still has prologue/epilogue
2. **NaN-boxing on return** — Even with optimizations, the return value needs boxing

### Optimization options (in order of ROI)

#### Option A: Tail-call optimization for tail-recursive patterns
Not directly applicable to `fib(n-1) + fib(n-2)`, but could help with accumulator-style rewrites.

#### Option B: Memoization detection
Compiler could detect pure recursive functions and auto-memoize. Complex to implement.

#### Option C: Loop unrolling for known small recursion depths
Inline the first 2-3 levels of recursion to reduce call overhead.

**Recommendation:** Accept the 1.2x gap for now — fibonacci is a micro-benchmark that doesn't reflect real-world patterns. The improvement from 2778ms to 1271ms is already excellent.

---

## P2: Improve Object Creation (2.4x slower)

**Current:** 12ms Perry vs 5ms Node (was 45ms before optimizations)

### Root cause analysis

The benchmark creates objects like:
```typescript
{
  id: i,
  name: 'item',
  value: i * 2,
  nested: { a: i, b: i * 2 }
}
```

The 3.75x improvement suggests `js_object_alloc_fast` is being used for some cases, but not all.

### Investigation needed

**File:** `crates/perry-codegen/src/codegen.rs`

Check:
1. Is `js_object_alloc_fast` used for the outer object?
2. Is it used for the nested `{ a: i, b: i * 2 }` object?
3. Are there other allocation paths being taken?

### Potential fix

The nested object literal may be falling back to `js_object_alloc` (slow path). Ensure the codegen uses `js_object_alloc_fast` for ALL object literals where every field has an initializer.

---

## P2: Improve Matrix Multiply (2.8x slower)

**Current:** 43ms Perry vs 15ms Node

### Root cause analysis

The benchmark is a triple-nested loop with array indexing:
```typescript
for (let i = 0; i < size; i++) {
  for (let j = 0; j < size; j++) {
    let sum = 0;
    for (let k = 0; k < size; k++) {
      sum = sum + a[i * size + k] * b[k * size + j];
    }
    c[i * size + j] = sum;
  }
}
```

Expected Perry to win here due to loop unrolling + BCE, but it's 2.8x slower.

### Investigation needed

**File:** `crates/perry-codegen/src/codegen.rs` — loop optimization passes

Check:
1. Is bounds check elimination (BCE) triggering for `a[i * size + k]`?
2. Is loop unrolling happening for the inner `k` loop?
3. Is the multiplication `i * size + k` being hoisted/optimized?

### Potential issues

1. **BCE not triggering** — The index expression `i * size + k` may be too complex for BCE to prove safe
2. **No loop unrolling** — The inner loop may not be unrolled because `size` is a runtime value
3. **Repeated index calculation** — `i * size` could be hoisted out of the `j` and `k` loops

### Fix approach

1. Add loop-invariant code motion (LICM) pass to hoist `i * size` computation
2. Consider partial BCE for patterns where index is bounded by loop counter
3. Profile the generated assembly to identify bottlenecks

---

## Execution Priority

| Priority | Task | Expected Impact | Effort |
|----------|------|-----------------|--------|
| **P0** | Fix `indexOf` crash | Unblocks 10x string-ops improvement | Medium |
| **P0** | Fix `split('')` crash | Enables idiomatic string patterns | Low |
| P1 | Accept fibonacci gap | N/A (already 2.2x better) | None |
| P2 | Debug object-create alloc path | Could close 2.4x → 1.5x gap | Medium |
| P2 | Investigate matrix-multiply codegen | Could close 2.8x → 1.5x gap | High |

---

## Verification Commands

After each fix:

```bash
# Rebuild Perry compiler
cd /Users/amlug/projects/perry && cargo build --release

# Rebuild demo
cd /Users/amlug/projects/perry-demo
/Users/amlug/projects/perry/target/release/perry src/perry-server.ts -o dist/perry-server

# Start server and test
PORT=3003 PERRY_RUNTIME=1 ./dist/perry-server &
curl -s 'http://localhost:3003/api/benchmarks/run/string-ops?iterations=100&size=10000'
curl -s 'http://localhost:3003/api/benchmarks/run/object-create?iterations=20&size=50000'
curl -s 'http://localhost:3003/api/benchmarks/run/matrix-multiply?iterations=10&size=200'
```
