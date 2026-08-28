# VSCode Compilation Feasibility Study - Experimental Results

> **Historical experiment (2026-01-20).** “Current main” below refers to the
> checkout used for that experiment, not today's compiler. Reproduce the study
> against a pinned VS Code revision before drawing a current compatibility
> conclusion.

## Summary

**Date:** 2026-01-20
**Perry Version:** Current main branch (with fixes applied)
**VSCode Version:** Latest (shallow clone)

**Result:** After implementing fixes for TsTypeAssertion, ??=/||=/&&= operators, comma operator, and 'in' operator, Perry can now compile simple VSCode files to object code. More complex files are blocked by generators, dynamic function calls, and function overloading.

---

## Files Tested (After Fixes)

| File | Result | Error Type | Progress |
|------|--------|------------|----------|
| `charCode.ts` | **PASS*** | Object file generated | Full lowering + codegen |
| `uint.ts` | **PASS*** | Object file generated | Full lowering + codegen |
| `types.ts` | FAIL | Function overloading | Past lowering, fails in codegen |
| `arrays.ts` | FAIL | Yield expression (generators) | Lowering stage |
| `strings.ts` | FAIL | Yield expression (generators) | Lowering stage |
| `assert.ts` | FAIL | Dynamic function call | Past TsTypeAssertion, new error |
| `lazy.ts` | FAIL | Dynamic method call | Same as before |
| `cache.ts` | FAIL | Dynamic new expression | Past ??=, new error |
| `errors.ts` | FAIL | Dynamic function call | Past TsTypeAssertion, new error |
| `event.ts` | FAIL | Dynamic new expression | Past ??=, new error |
| `lifecycle.ts` | FAIL | Unsupported pattern | Same as before |
| `async.ts` | FAIL | Dynamic new expression | Past TsTypeAssertion, new error |

\* These files successfully generate object files but fail at link time because they're library modules without a `main` entry point.

---

## Fixes Applied

The following fixes were implemented during this study:

### 1. TsTypeAssertion (Angle Bracket Syntax) ✅ FIXED
```rust
// crates/perry-hir/src/lower.rs
ast::Expr::TsTypeAssertion(ts_assertion) => {
    lower_expr(ctx, &ts_assertion.expr)
}
```

### 2. Comma Operator (Seq Expression) ✅ FIXED
```rust
// crates/perry-hir/src/lower.rs
ast::Expr::Seq(seq) => {
    let mut last_expr = Expr::Undefined;
    for expr in &seq.exprs {
        last_expr = lower_expr(ctx, expr)?;
    }
    Ok(last_expr)
}
```

### 3. Logical Assignment Operators (&&=, ||=, ??=) ✅ FIXED
```rust
// crates/perry-hir/src/lower.rs
ast::AssignOp::NullishAssign => {
    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
    Box::new(Expr::Logical {
        op: LogicalOp::Coalesce,
        left,
        right: Box::new(rhs),
    })
}
```

### 4. 'in' Operator ✅ FIXED
```rust
// crates/perry-hir/src/ir.rs - Added new expression type
In {
    property: Box<Expr>,
    object: Box<Expr>,
},

// crates/perry-hir/src/lower.rs
if matches!(bin.op, ast::BinaryOp::In) {
    let property = Box::new(lower_expr(ctx, &bin.left)?);
    let object = Box::new(lower_expr(ctx, &bin.right)?);
    return Ok(Expr::In { property, object });
}
```

---

## Remaining Blockers (New Errors After Fixes)

### 1. Generator Functions / Yield (HIGH priority)
**Files:** arrays.ts, strings.ts

Generator functions (`function*`) and `yield` expressions are not supported. This requires:
- State machine transformation at compile time
- Runtime iterator protocol support
- `Symbol.iterator` implementation

### 2. Dynamic Function Calls (MEDIUM priority)
**Files:** assert.ts, errors.ts, lazy.ts

Calling a variable as a function (e.g., `callback()` where `callback` is a parameter) isn't fully supported. The error is "Unsupported callee: LocalGet that is not a closure".

### 3. Dynamic New Expressions (MEDIUM priority)
**Files:** cache.ts, event.ts, async.ts

`new SomeExpression()` where the expression isn't a simple identifier fails with "New expression callee must be an identifier".

### 4. Function Overloading (LOW priority for now)
**Files:** types.ts

TypeScript function overloading (multiple declarations with different arities) causes codegen signature conflicts.

### 5. Destructuring Patterns (MEDIUM priority)
**Files:** lifecycle.ts

Some destructuring patterns are not fully implemented.

---

## Specific Failures (Original Analysis)

### 1. TsTypeAssertion (Angle Bracket Syntax)

**Files affected:** types.ts, assert.ts, errors.ts, async.ts

**Example code:**
```typescript
return validValues.includes(<TSubtype>value);  // line 211 of types.ts
```

**Error:**
```
Error: Unsupported expression type: TsTypeAssertion(...)
```

**Why it fails:**
- Perry handles `value as Type` (TsAs) but not `<Type>value` (TsTypeAssertion)
- Both are equivalent at runtime - the expression should just pass through

**Fix location:** `crates/perry-hir/src/lower.rs` line ~4425

**Proposed fix:**
```rust
ast::Expr::TsTypeAssertion(ts_assertion) => {
    // Same as TsAs - just evaluate the expression, type is compile-time only
    lower_expr(ctx, &ts_assertion.expr)
}
```

---

### 2. Yield Expression (Generators)

**Files affected:** arrays.ts

**Example code:**
```typescript
function* groupBy<T>(items: Iterable<T>) {
    yield currentGroup;  // line ~150
}
```

**Error:**
```
Error: Unsupported expression type: Yield(YieldExpr { ... })
```

**Why it fails:**
- Perry has no support for generator functions (`function*`)
- Would require significant runtime support for generator state machines

**Fix complexity:** HIGH - Requires:
1. New `Expr::Yield` variant in HIR
2. Generator function lowering (state machine transformation)
3. Runtime iterator protocol support
4. `Symbol.iterator` implementation

---

### 3. Comma Operator (Seq Expression)

**Files affected:** strings.ts

**Example code:**
```typescript
for (...; aStart++, bStart++) { ... }  // line ~315
```

**Error:**
```
Error: Unsupported expression type: Seq(SeqExpr { exprs: [Update(...), Update(...)] })
```

**Why it fails:**
- The comma operator evaluates multiple expressions left-to-right, returning the last value
- Perry doesn't handle `SeqExpr`

**Fix location:** `crates/perry-hir/src/lower.rs`

**Proposed fix:**
```rust
ast::Expr::Seq(seq) => {
    // Evaluate all expressions, return the last one's value
    let mut stmts = Vec::new();
    let exprs: Vec<_> = seq.exprs.iter()
        .map(|e| lower_expr(ctx, e))
        .collect::<Result<Vec<_>>>()?;

    // For HIR, we can wrap all but last in ExprStmt, then return last
    // Or add Expr::Seq to HIR
    if exprs.is_empty() {
        Ok(Expr::Undefined)
    } else if exprs.len() == 1 {
        Ok(exprs.into_iter().next().unwrap())
    } else {
        Ok(Expr::Seq(exprs))  // Need to add this variant
    }
}
```

---

### 4. Nullish Coalescing Assignment (??=)

**Files affected:** cache.ts, event.ts

**Example code:**
```typescript
this._value ??= this._fn();
```

**Error:**
```
Error: Unsupported assignment operator: "??="
```

**Why it fails:**
- Modern assignment operators (??=, ||=, &&=) not implemented
- `a ??= b` means `a = a ?? b` but only evaluates `b` if `a` is null/undefined

**Fix location:** `crates/perry-hir/src/lower.rs` in assignment handling

**Proposed fix:**
```rust
// In the assignment operator handling:
"??=" => {
    // a ??= b  →  a = (a ?? b)
    Ok(Expr::Assign {
        target: lower_assign_target(ctx, &assign.left)?,
        value: Box::new(Expr::NullishCoalesce {
            left: Box::new(lower_assign_target_as_expr(ctx, &assign.left)?),
            right: Box::new(lower_expr(ctx, &assign.right)?),
        }),
    })
}
```

---

### 5. Unsupported Method Call Pattern

**Files affected:** lazy.ts

**Example code:**
```typescript
const value = executor();  // Where executor is a parameter
```

**Error:**
```
Error: Unsupported method call: executor
```

**Why it fails:**
- Perry's call handling expects known patterns (direct function calls, method calls)
- Calling a variable that holds a function reference isn't fully supported

**Fix location:** `crates/perry-hir/src/lower.rs` in call expression handling

---

### 6. Unsupported Pattern (Destructuring)

**Files affected:** lifecycle.ts

**Error:**
```
Error: Unsupported pattern
```

**Why it fails:**
- Some destructuring patterns aren't fully implemented
- Could be rest patterns, nested destructuring, or default values

---

## Files That Successfully Lower

Two files passed the lowering phase:

1. **charCode.ts** - A `const enum` with numeric values only
2. **uint.ts** - Simple constant definitions

Both failed at linking because they have no `main` function (they're library modules).

This indicates Perry's basic enum/const support works for simple cases.

---

## Priority Fix Order

Based on frequency in VSCode and implementation complexity:

| Priority | Feature | Occurrences | Complexity | Time Estimate |
|----------|---------|-------------|------------|---------------|
| 1 | TsTypeAssertion | 4 files | LOW | 1 hour |
| 2 | ??= operator | 2 files | LOW | 2 hours |
| 3 | Comma operator (Seq) | 1 file | LOW | 2 hours |
| 4 | Variable function calls | 1 file | MEDIUM | 4 hours |
| 5 | Destructuring patterns | 1 file | MEDIUM | 1 day |
| 6 | Generators/Yield | 1 file | HIGH | 1-2 weeks |

---

## Recommended Next Steps

### Quick Wins (Today)

1. **Add TsTypeAssertion support** - Single line fix in lower.rs
2. **Add ??= operator** - Desugar to nullish coalescing
3. **Add Seq expression** - Evaluate all, return last

### Medium Term (This Week)

4. **Improve call expression handling** - Support calling any expression
5. **Complete destructuring patterns** - Handle rest, defaults

### Longer Term

6. **Generator support** - Full state machine transformation
7. **Library module compilation** - Don't require main()

---

## Code Locations for Fixes

All TypeScript language fixes go in:
```
crates/perry-hir/src/lower.rs
```

The main `lower_expr` function starts around line 2629 and the catch-all error is at line 4430:
```rust
_ => Err(anyhow!("Unsupported expression type: {:?}", expr)),
```

New expression types need corresponding variants in:
```
crates/perry-hir/src/ir.rs
```

---

## Conclusion

**VSCode cannot be compiled with Perry today**, but the gaps are well-defined:

1. **TypeScript Language Features** - Missing syntax support (TsTypeAssertion, generators, etc.)
2. **Advanced Operators** - ??=, comma operator
3. **Call Patterns** - Calling variables as functions

The first three quick fixes (TsTypeAssertion, ??=, Seq) would likely unlock several more files. Generator support is the largest missing feature that VSCode uses heavily.

This experiment confirms the feasibility study's assessment: TypeScript language features are the primary blocker, not runtime APIs (which weren't even reached).
