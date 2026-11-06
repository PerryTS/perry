// Issue #431 — sibling fixture for the cross-module class-name collision.
// b.ts and the consumer (../../test_issue_431_cross_module_class_collision.ts)
// each declare a class named `DroppingStrategy`. Pre-fix, perry's codegen
// emitted the consumer's method bodies under THIS file's module prefix
// (whichever same-named class registration won the race in
// `imported_class_prefix`), and the consumer's dispatch table referenced
// them under its own prefix — clang rejected the IR with `use of
// undefined value '@perry_method_<consumer>_ts__DroppingStrategy__*'`.

const BTypeId: unique symbol = Symbol.for("BTypeId-issue-431");
const bVar = { _A: (_: never) => _ };

export class DroppingStrategy<in out A> {
  readonly [BTypeId] = bVar;
  surplusSize(): number {
    return 1;
  }
  handleSurplus(elements: Array<A>): boolean {
    return elements.length > 0;
  }
}

// `function` form (not arrow) — sidesteps an orthogonal arrow-export bug
// (`export const f = () => ...` printed as `[Function (anonymous)]`).
// What's relevant for #431 is the cross-module class collision, which
// surfaces at compile time regardless of how the bridge function is
// declared.
export function useStrategy(): boolean {
  const b = new DroppingStrategy<number>();
  return b.handleSurplus([1, 2, 3]);
}

export function probeSize(): number {
  const b = new DroppingStrategy<string>();
  return b.surplusSize();
}
