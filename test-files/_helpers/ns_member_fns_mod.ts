// Helper for test_gap_namespace_member_not_array_method.ts (#321 / #24).
// A module that happens to export functions named like array methods
// (`map`, `filter`, ...) — exactly effect's `export const map = core.map`
// shape — plus a real array export.

export const map = (x: number, f: (n: number) => number): number => f(x);
export const filter = (x: number, p: (n: number) => boolean): boolean => p(x);
export const find = (x: number): number => x + 1;

// A real exported array, to confirm `NS.items.map(cb)` still array-maps.
export const items = [10, 20, 30];

// #39 (#321): effect's `Array.sort = dual(2, body)` — a data-last (pipeable)
// export named `sort`. Called with ONE arg (`NS.sort(cmp)`) it must return the
// curried `(self) => sorted` closure, NOT fold to the `js_array_sort`
// intrinsic on the namespace object (which returned the namespace itself,
// typeof "object", and threw "value is not a function" downstream in
// `pipe(bounds, sortFn, ...)`). `sort` is NOT in the array-method
// `is_overlapping` set, so it bypassed the imported-array guard and reached
// `try_array_only_methods`, which folded it to `Expr::ArraySort`.
const dual = function (arity: number, body: any): any {
  if (arity === 2) {
    return function (a: any, b: any) {
      // eslint-disable-next-line prefer-rest-params
      if (arguments.length >= 2) {
        return body(a, b);
      }
      return function (self: any) {
        return body(self, a);
      };
    };
  }
};

export const sort: {
  <B>(O: (a: B, b: B) => number): <A extends B>(self: Array<A>) => Array<A>;
  <A extends B, B>(self: Array<A>, O: (a: B, b: B) => number): Array<A>;
} = dual(2, <A extends B, B>(self: Array<A>, O: (a: B, b: B) => number): Array<A> => {
  const out = Array.from(self);
  out.sort(O);
  return out;
});
