# Perry Complex TypeScript Stress Test Findings

> **Historical test snapshot (2026-02-16).** The five `test_complex_*` files
> named below have since been removed or replaced and these pass/fail counts
> must not be used as current compatibility evidence. Current behavioral
> coverage lives in the registered gap/parity suites and
> [`src/testing/test-registration.md`](src/testing/test-registration.md).

Date: February 16, 2026

## Summary
A new stress suite was added to exercise broad TypeScript/JavaScript semantics in Perry.  
When run locally, Node passes all tests, while Perry shows both runtime crashes and semantic mismatches.

## Test Files
- `test-files/test_complex_vars.ts`
- `test-files/test_complex_functions_closures.ts`
- `test-files/test_complex_classes_objects.ts`
- `test-files/test_complex_runtime_probes.ts`
- `test-files/test_complex_perry_showcase.ts`

## Reproduction
### Perry
```bash
target/debug/perry compile test-files/test_complex_vars.ts -o /tmp/test_complex_vars && /tmp/test_complex_vars
target/debug/perry compile test-files/test_complex_functions_closures.ts -o /tmp/test_complex_functions_closures && /tmp/test_complex_functions_closures
target/debug/perry compile test-files/test_complex_classes_objects.ts -o /tmp/test_complex_classes_objects && /tmp/test_complex_classes_objects
target/debug/perry compile test-files/test_complex_runtime_probes.ts -o /tmp/test_complex_runtime_probes && /tmp/test_complex_runtime_probes
target/debug/perry compile test-files/test_complex_perry_showcase.ts -o /tmp/test_complex_perry_showcase && /tmp/test_complex_perry_showcase
```

### Node baseline
```bash
node --experimental-transform-types test-files/test_complex_vars.ts
node --experimental-transform-types test-files/test_complex_functions_closures.ts
node --experimental-transform-types test-files/test_complex_classes_objects.ts
node --experimental-transform-types test-files/test_complex_runtime_probes.ts
node --experimental-transform-types test-files/test_complex_perry_showcase.ts
```

## Results Comparison
| File | Perry Build | Perry Run | Perry Outcome | Node Outcome |
|---|---:|---:|---|---|
| `test_complex_vars.ts` | 0 | 0 | `SUMMARY failures=0` | `SUMMARY failures=0` |
| `test_complex_functions_closures.ts` | 0 | 132 | Runtime crash (no summary) after initial passes | `SUMMARY failures=0` |
| `test_complex_classes_objects.ts` | 0 | 0 | `SUMMARY failures=8` | `SUMMARY failures=0` |
| `test_complex_runtime_probes.ts` | 0 | 0 | `SUMMARY failures=2` | `SUMMARY failures=0` |
| `test_complex_perry_showcase.ts` | 0 | 139 | Runtime crash (no summary) | `SUMMARY failures=0 checks=51` |

## Failing Checks (Perry)
### `test_complex_classes_objects.ts` (8 failures)
- `inheritance override`
- `instance method this`
- `static field`
- `method with receiver`
- `object rest count`
- `destructure rest len`
- `destructure dropped key`
- `union circle`

### `test_complex_runtime_probes.ts` (2 failures)
- `dynamic key count`
- `delete removes key`

### Crash Signals
- `test_complex_functions_closures.ts`: exit code `132` (illegal instruction)
- `test_complex_perry_showcase.ts`: exit code `139` (segmentation fault)

## Impact
These failures indicate that core runtime semantics are inconsistent with JavaScript in several critical areas:
- Class/inheritance and method dispatch
- `this` binding behavior
- Object rest/destructuring handling
- Union/math behavior consistency
- Dynamic property table growth and key tracking
- `delete` operator semantics on objects
- Runtime stability under broader feature combinations

## Recommended Priority
1. Stabilize runtime crashes (`132`, `139`) first.
2. Fix class and `this` semantics (high user-visible correctness impact).
3. Fix dynamic object key tracking and `delete` semantics.
4. Re-run this suite in CI as a regression gate.

## Notes
- `test_complex_perry_showcase.ts` intentionally combines many features and acts as an integration stress test.
- Focused files isolate specific semantic domains for faster debugging.
