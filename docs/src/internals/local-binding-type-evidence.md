# Local binding type evidence

Perry compiles TypeScript, but TypeScript annotations do not exist at runtime.
The compiler must therefore distinguish three statements that look similar in
source code:

1. a binding is *declared* as `T`;
2. a binding's initializer happened to produce `T`; and
3. the value at this use site is proven to have representation `T`.

Only the third statement can license an unguarded specialized lowering or a
compile-time answer. An initializer can establish it only when the expression's
own runtime semantics fix the kind and no later write can replace the value.

## The invariant

> A type or kind fact attached to a local binding is usable only if every write
> that could invalidate it is excluded. A declared TypeScript type is never a
> runtime representation proof by itself.

A later assignment is the invalidating event. The current implementation uses
a conservative whole-region rule: if a binding is assigned anywhere after its
declaration, initializer-derived evidence disappears for the whole region.
Declared annotations never enter the proof map. This can miss an optimization
before the write, but it cannot let a non-dominating write silently change the
meaning of a later operation.

This is intentionally stricter than statement-order tracking. A future CFG
analysis may recover hints at sites dominated by a validating guard, but the
fallback must remain the runtime path.

## Codegen APIs

`FnCtx` exposes two reads:

- `stable_local_type_proof(id)` reads a separate map populated only by
  initializer expressions whose runtime semantics establish their outer kind,
  and returns no type when the region contains a write to `id`.
- `local_type_hint(id)` is the narrow escape hatch for a consumer with an
  independent representation proof or a runtime guard that validates the
  current value. It deliberately preserves the hint across assignment.

Initializer proofs are deliberately syntax-based. Literal primitives,
primitive operators whose operands are already proven, array/object literals,
closures, and the outer Object result of `new` qualify. A specialized method
HIR node does not: it may retain an override-aware fallback with a different
result kind. Class identity and generic element types are likewise not inferred
from the outer allocation alone.

Examples of valid escape-hatch uses include a typed-array runtime helper that
checks the receiver's actual GC kind, a scalar clone entered behind a public tag
guard, and a buffer-view slot whose pointer state every write invalidates.

Examples that are not valid proofs include `number`, `boolean`, or `string` on
a local declaration; a property or function return type propagated from an
annotation; and a method name guessed without validating the receiver. Nested
generic claims are erased: an intrinsic can prove Array without proving the
declared types of its future elements.

The precise-GC pointer collector follows the same rule independently. It roots
every generic-ABI parameter and every local until the complete write set proves
that all values are non-pointers. A scalar annotation can therefore never
suppress a root for an object actually stored in that binding. Typed closure
capture annotations are only candidates: both the public trampoline and the
direct local-call path validate every current capture slot and branch to the
generic body on failure.

Module globals used by worker-thread admission have a separate structural
initializer map and a module-wide write check. Missing evidence is hazardous;
a transferable-looking annotation cannot make an arbitrary main-heap value
safe to read from another worker arena.

For operator selection, use the stronger per-value predicates where available.
For example, `string_value_is_runtime_guaranteed` separates a value constructed
as a string from one that is merely declared as a string. Bare local truthiness
always uses `js_is_truthy`, because numeric and boolean annotations can hold
any NaN-boxed value.

## Static inventory and CI gate

`scripts/local_binding_type_audit.py` scans production HIR/codegen sources for
both accessors and for remaining lower-level type-map reads. Its count-exact
inventory is `scripts/local_binding_type_allowlist.json`.

Each inventory group states one of three verdicts:

- `runtime-validated`: emitted code checks the current runtime value;
- `representation-proven`: another analysis establishes the storage fact and
  invalidates it on writes;
- `metadata-only`: the type read cannot select a runtime answer or layout.

The gate fails when a consumer is added without a verdict, when a count changes,
when an entry matches nothing, or when code bypasses the accessors with a direct
read of the hint or proof maps. Its self-test plants both bypass forms, all
inventory drift modes, and an empty scan, so a stale scanner cannot report a
vacuous success.

Run it locally with:

```sh
python3 scripts/local_binding_type_audit.py --self-test
python3 scripts/local_binding_type_audit.py
python3 scripts/local_binding_type_audit.py --list
```
