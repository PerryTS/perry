//! `toReversed` / `toSorted` / `toSpliced` / `with`, `copyWithin` and `reduceRight` on
//! the web target.

use super::run_web_source;

// Each method's result and the receiver after it, on a module-level array and on one
// local to a function; `toSpliced` inserts its items. `reduceRight` rejects a non-callable
// callback and an empty array without an initial value, takes an explicit undefined as
// one and skips absent elements. Expected output is Node's.
#[test]
fn array_copy_methods_match_node() {
    assert_eq!(
        run_web_source(
            r#"
const a = [3, 1, 2];
console.log(a.toReversed().join(), a.toSorted().join(), a.toSorted((x, y) => y - x).join(), a.join());
console.log(a.toSpliced(1, 1, 9, 8).join(), a.toSpliced(1).join(), a.with(0, 7).join(), a.join());
const b = [1, 2, 3, 4, 5];
console.log(b.copyWithin(0, 3).join(), b.join());
function local() {
  const c = [1, 2, 3, 4, 5];
  c.copyWithin(1, 0, 2);
  return c.join();
}
console.log(local());
console.log([1, 2, 3].reduceRight((acc, x) => acc + x, ""), [1, 2, 3].reduceRight((acc, x) => acc * 10 + x));
const tries = (f: () => any) => { try { return String(f()); } catch (e: any) { return e.name; } };
console.log(tries(() => [1].reduceRight(null as any, 0 as any)), tries(() => [].reduceRight((x: any) => x)), tries(() => [].reduceRight((x: any) => x, 5 as any)));
const sparse: any[] = [1, 2, 3];
delete sparse[1];
console.log(([1] as any[]).reduceRight(() => 42, undefined), sparse.reduceRight((acc, x) => acc + "," + x));
const del: any[] = [1, 2, 3];
console.log(del.reduceRight((acc, _x, i, arr) => { if (i === 2) delete arr[0]; return acc + 1; }, 0));
"#
        ),
        "2,1,3 1,2,3 3,2,1 3,1,2\n3,9,8,2 3 7,1,2 3,1,2\n4,5,3,4,5 4,5,3,4,5\n1,1,2,4,5\n321 321\nTypeError TypeError 5\n42 3,1\n2"
    );
}
