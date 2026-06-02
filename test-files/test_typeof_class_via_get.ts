// Refs v0.5.745: typeof of class ref via PropertyGet/LocalGet should
// return "function" (not "number"). Class refs are stored as INT32-tag
// NaN-boxed (`INT32_TAG | class_id`), and the runtime's js_value_typeof
// is_int32 branch was returning "number" for them. The fix consults a
// runtime registry of known class ids registered at module init.
class SQL {
    static kind = "SQL";
}

((S: any) => {
    class Aliased {
        static kind = "Aliased";
    }
    S.Aliased = Aliased;
})(SQL);

console.log("typeof Aliased direct:", typeof (SQL as any).Aliased);
console.log("typeof SQL:", typeof SQL);
const A = (SQL as any).Aliased;
console.log("typeof A:", typeof A);
