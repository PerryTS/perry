// Refs v0.5.757: `((CLS2) => { CLS2.X = ...; })(CLS || (CLS = {} as any))`
// — drizzle-orm's sql.js IIFE pattern for adding `static [Symbol]`-ish
// dynamic properties to a class. Pre-fix two issues blocked this:
//   (a) `CLS = {} as any` (assignment to a class binding) was treated
//       as an "implicit local declaration" by HIR lowering, hiding the
//       original class binding from subsequent reads — `CLS.X` then
//       returned undefined because the local was zero-init'd.
//   (b) `CLS.X` reading via `Expr::PropertyGet { Expr::ExternFuncRef("CLS"),
//       "X" }` (i.e. an IMPORTED class ref) only consulted the static-field
//       globals map; for properties added dynamically via the IIFE pattern,
//       the read fell through to the PIC fast path which discards the
//       INT32 NaN-tag during unbox and ended up returning undefined.
class SQL {
    static x = "static-x";
}

((SQL2: any) => {
    class Aliased {
        foo: string;
        constructor(foo: string) { this.foo = foo; }
    }
    SQL2.Aliased = Aliased;
})(SQL || (SQL = {} as any));

console.log("typeof SQL:", typeof SQL);
console.log("typeof SQL.Aliased:", typeof (SQL as any).Aliased);
console.log("SQL.x:", (SQL as any).x);

// Also try with an OR-self-init form via let-binding.
let sqlFn: any = function sql() { return "sql-call"; };

((s: any) => {
    s.helper = "helper-set";
})(sqlFn || (sqlFn = {}));

console.log("typeof sqlFn:", typeof sqlFn);
console.log("sqlFn.helper:", sqlFn.helper);
