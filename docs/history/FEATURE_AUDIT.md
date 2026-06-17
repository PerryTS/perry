# Compilets Feature Audit

**Date:** January 2026
**Version:** Current development state
**Test Status:** 62/62 tests passing

## Overview

Compilets is a native TypeScript compiler that compiles TypeScript to native executables using SWC (parsing) and LLVM (code generation).

Run `./run_tests.sh` to execute the full test suite.

---

## Working Features ✅

### Core Language

| Feature | Status | Notes |
|---------|--------|-------|
| Numbers (f64) | ✅ Full | All arithmetic (+, -, *, /, %) work |
| Booleans | ✅ Full | true/false, comparison, logical operators |
| Strings | ✅ Partial | Literals, concatenation, length, indexOf work |
| Variables | ✅ Full | let/const declarations |
| Unary operators | ✅ Full | Negation (-x), logical not (!x) |
| Ternary operator | ✅ Full | condition ? a : b |

### Control Flow

| Feature | Status | Notes |
|---------|--------|-------|
| If/else | ✅ Full | All branching patterns |
| While loops | ✅ Full | Including break/continue |
| For loops | ✅ Full | Including break/continue |
| Break | ✅ Full | Works in all loops |
| Continue | ✅ Full | Works in all loops |

### Functions

| Feature | Status | Notes |
|---------|--------|-------|
| Function declaration | ✅ Full | Named functions |
| Function calls | ✅ Full | Direct calls |
| Recursion | ✅ Full | Self-referential calls |
| Parameters | ✅ Full | Multiple parameters |
| Return values | ✅ Full | Explicit returns |

### Classes

| Feature | Status | Notes |
|---------|--------|-------|
| Class declaration | ✅ Full | Basic class syntax |
| Constructors | ✅ Full | With parameters |
| Instance fields | ✅ Full | this.field access |
| Instance methods | ✅ Full | Method calls on instances |
| Class inheritance | ✅ Full | extends keyword |
| super() calls | ✅ Full | Parent constructor calls |

### Arrays

| Feature | Status | Notes |
|---------|--------|-------|
| Array literals | ✅ Full | [1, 2, 3] syntax |
| Array indexing | ✅ Full | arr[i] read/write |
| Array length | ✅ Full | arr.length property |

### Other

| Feature | Status | Notes |
|---------|--------|-------|
| Enums | ✅ Full | Numeric and string enums |
| BigInt | ✅ Full | 256-bit integers |
| Simple closures | ✅ Partial | Arrow functions work locally |
| console.log | ✅ Full | Numbers and strings |
| setTimeout | ✅ Full | Async timer support |

---

## Not Working / Missing ❌

### Operators

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| ++/-- operators | ✅ Full | - | Pre/post increment/decrement work |
| Compound assignment | ✅ Full | - | +=, -=, *=, /=, %=, **=, &=, |=, ^=, <<=, >>=, >>>= all work |
| Bitwise operators | ✅ Full | - | &, |, ^, ~, <<, >>, >>> all work |
| ** (exponent) | ✅ Full | - | 2 ** 3 = 8, also **= compound assignment |

### Strings

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| string.slice() | ✅ Full | - | Returns substring from start to end index |
| string.substring() | ✅ Full | - | Works (similar to slice but swaps args if start > end) |
| string.split() | ✅ Full | - | Split by delimiter, returns string[] |
| string.trim() | ✅ Full | - | Remove whitespace from both ends |
| string.toLowerCase() | ✅ Full | - | Convert to lowercase |
| string.toUpperCase() | ✅ Full | - | Convert to uppercase |
| Template literals | ✅ Full | - | Backtick strings with ${expr} interpolation |

### Arrays

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| push/pop | ✅ Full | - | Array mutation methods |
| shift/unshift | ✅ Full | - | Array mutation methods |
| indexOf/includes | ✅ Full | - | Search methods |
| map/filter/reduce | ✅ Full | - | Functional array methods |
| forEach | ✅ Full | - | Iteration method |
| slice | ✅ Full | - | Array slicing (slice(start, end?)) |
| splice | ✅ Full | - | Array splice (splice(start, deleteCount?, ...items)) |
| Mixed-type arrays | ✅ Full | - | Arrays can hold mixed types (string\|number)[] using NaN-boxing |

### Classes

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| Method inheritance | ✅ Full | - | Methods inherited and can be overridden |
| Static methods | ✅ Full | - | Class-level methods |
| Static fields | ✅ Full | - | Class-level properties |
| Getters/setters | ✅ Full | - | get/set accessors work |
| Private fields (#) | ✅ Full | - | ES2022 private class fields (#field) |

### Functions/Closures

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| Closures as args | ✅ Full | - | Arrow functions work as callback arguments |
| Higher-order functions | ✅ Full | - | Named functions and closures work as callbacks |
| Returning closures | ✅ Full | - | Mutable captures work when returned; use type inference for closure return types |
| Rest parameters | ✅ Full | - | ...args syntax collects remaining arguments into array |
| Default parameters | ✅ Full | - | Parameters with default values |

### Control Flow

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| Try-catch-finally | ✅ Full | - | Works with throw, catch, finally |
| Switch statement | ✅ Full | - | With fallthrough support |
| For-of loops | ✅ Full | - | Array iteration |
| For-in loops | ✅ Full | - | Object key iteration |

### Type System

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| Type extraction | ✅ Full | - | Types extracted from annotations |
| Generics | ✅ Full | - | Monomorphization generates specialized functions/classes |
| Interfaces | ✅ Full | - | Interface declarations with properties and methods |
| Type aliases | ✅ Full | - | type X = ... declarations |
| Constraint checking | ✅ Full | - | T extends Foo constraints validated |
| Type guards | ✅ Full | - | typeof operator and type narrowing with union types |
| Union types | ✅ Full | - | Variables with string|number work; console.log handles both types dynamically |

### Modules

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| import/export | ✅ Full | - | Named imports/exports, re-exports work |
| require() | ✅ Full | - | CommonJS require() for built-in modules (fs, path, crypto) |

### Standard Library

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| fs module | ✅ Full | - | readFileSync, writeFileSync, existsSync, mkdirSync, unlinkSync |
| path module | ✅ Full | - | join, dirname, basename, extname, resolve |
| process.env | ✅ Full | - | Environment variables (process.env.VARNAME) |
| crypto | ✅ Full | - | randomBytes, randomUUID, sha256, md5 |
| Date | ✅ Full | - | Date.now(), new Date(), getTime(), toISOString(), component getters |
| JSON | ✅ Full | - | JSON.parse and JSON.stringify |
| Math | ✅ Full | - | floor, ceil, round, abs, sqrt, pow, min, max, random |
| Map/Set | ✅ Full | - | Map and Set collection types |
| RegExp | ✅ Full | - | string.replace(), regex.test(), string.match() (global and non-global) |

### Other

| Feature | Status | Priority | Notes |
|---------|--------|----------|-------|
| Destructuring | ✅ Full | - | Array and object destructuring (shorthand, rename, defaults, rest), including nested and rest patterns |
| Spread operator | ✅ Full | - | [...arr, x] array spread syntax |
| Optional chaining | ✅ Full | - | obj?.prop, obj?.[index] |
| Nullish coalescing | ✅ Full | - | ?? operator |
| Method chaining | ✅ Full | - | Array/string method chains (e.g. arr.filter().map(), str.toLowerCase()) |
| Utility types | ✅ Full | - | Partial, Pick, Record, Omit, ReturnType, Readonly erased at compile time |
| Decorators | ⚠️ Partial | - | @log method decorator (compile-time transformation) |

---

## Test Files

The following test files are available in `test-files/`:

| File | Tests |
|------|-------|
| test_for.ts | For loops |
| test_for_in.ts | For-in loops (object key iteration) |
| test_break_continue.ts | Break and continue |
| test_enum.ts | Enum support |
| test_inheritance.ts | Class inheritance |
| test_simple_class.ts | Basic classes |
| test_closure_complex.ts | Closure support |
| test_timer.ts | setTimeout/async |
| test_mutable_capture.ts | Mutable closure captures |
| test_returning_closures.ts | Returning closures from functions |
| test_process_env.ts | process.env access |
| test_fs.ts | fs module (read/write/exists/mkdir/unlink) |
| test_path.ts | path module (join/dirname/basename/extname/resolve) |
| test_array_methods.ts | Array methods (push/pop/shift/unshift/indexOf/includes) |
| test_string_methods.ts | String methods (trim/toLowerCase/toUpperCase) |
| test_json.ts | JSON.parse and JSON.stringify |
| test_math.ts | Math functions (floor, ceil, round, abs, sqrt, pow, min, max, random) |
| test_crypto.ts | Crypto functions (randomBytes, randomUUID, sha256, md5) |
| test_date.ts | Date functions (Date.now, new Date, getTime, toISOString, getFullYear, etc.) |
| test_bitwise.ts | Bitwise operators (&, |, ^, ~, <<, >>, >>>, and compound assignments) |
| test_regex.ts | RegExp support (test(), string.match(), string.replace()) |
| test_object_destructuring.ts | Object destructuring (shorthand, rename, defaults, rest) |
| test_method_chaining.ts | Method chaining (arr.filter().map(), string chains) |
| test_utility_types.ts | TypeScript utility type erasure (Partial, Pick, Record, Omit, etc.) |
| test_rest_params.ts | Rest parameters (...args syntax) |
| test_getters_setters.ts | Class getters and setters |
| test_private_fields.ts | Private class fields (#field syntax) |
| test_union_types.ts | Union types (string \| number variables) |
| test_type_guards.ts | Type guards (typeof operator, type narrowing) |
| test_mixed_arrays.ts | Mixed-type arrays ((string\|number)[]) |
| test_require.ts | CommonJS require() for built-in modules |
| test_decorators.ts | Method decorators (@log) |
| test_integration_app.ts | Integration: classes, decorators, file I/O, arrays |
| test_data_pipeline.ts | Integration: generics, Map/Set, array methods, Math |
| test_cli_simulation.ts | Integration: fs, path, strings, try-catch, JSON |

---

## Recommended Implementation Order

### Phase 1: Core Language Completeness
1. ~~**++/-- operators**~~ - ✅ DONE
2. ~~**Compound assignment (+=, etc)**~~ - ✅ DONE
3. ~~**Method inheritance**~~ - ✅ DONE

### Phase 2: Error Handling & Modules
4. ~~**Try-catch-finally**~~ - ✅ DONE (already implemented!)
5. ~~**Module imports**~~ - ✅ DONE (already implemented!)

### Phase 3: Standard Library
6. ~~**process.env**~~ - ✅ DONE
7. ~~**fs module**~~ - ✅ DONE
8. ~~**path module**~~ - ✅ DONE

### Phase 4: Advanced Features
9. ~~**Generics**~~ - ✅ DONE (monomorphization, interfaces, type aliases, constraints)
10. ~~**Array methods**~~ - ✅ DONE (push, pop, shift, unshift, indexOf, includes)

### Phase 5: Functional Programming
11. ~~**Higher-order functions**~~ - ✅ DONE (closures and named functions as callbacks)
12. ~~**Array HOF methods**~~ - ✅ DONE (map, filter, reduce, forEach)

### Phase 6: Modern Syntax
13. ~~**Switch statements**~~ - ✅ DONE (with fallthrough support)
14. ~~**For-of loops**~~ - ✅ DONE (array iteration)
15. ~~**Destructuring**~~ - ✅ DONE (array destructuring with nested and rest patterns)
16. ~~**Spread operator**~~ - ✅ DONE (...array syntax)
17. ~~**Template literals**~~ - ✅ DONE (backtick strings with ${expr})
18. ~~**Default parameters**~~ - ✅ DONE (function parameters with defaults)
19. ~~**Optional chaining**~~ - ✅ DONE (obj?.prop, obj?.[index])
20. ~~**Nullish coalescing**~~ - ✅ DONE (?? operator)

---

## Runtime Notes

- **No garbage collection** - Memory leaks in long-running code
- **Arrays are f64-only** - No mixed-type arrays
- **Dynamic array growth** - Arrays grow automatically via push/unshift
- **Async via event loop** - setTimeout works with await
