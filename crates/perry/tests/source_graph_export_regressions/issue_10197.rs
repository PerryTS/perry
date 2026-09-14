//! `ns.member(...spread)` on a `const`-bound rest-parameter export must call
//! the closure, not evaluate to it (#10197).

use super::{compile_and_run, write};

fn lib(dir: &std::path::Path) {
    write(
        dir,
        "lib.ts",
        "export const count = (...xs: any[]) => xs.length\n\
         export const mergeAll = (...ctxs: any[]) => {\n\
         \x20 const m = new Map()\n\
         \x20 for (let i = 0; i < ctxs.length; i++) ctxs[i].mapUnsafe.forEach((v: any, k: any) => m.set(k, v))\n\
         \x20 return { mapUnsafe: m }\n\
         }\n\
         export function countFn(...xs: any[]) { return xs.length }\n\
         export const mk = (k: string) => ({ mapUnsafe: new Map([[k, k.length]]) })\n\
         export const fill = (n: number, f: (i: number) => any) => { const out = new Array(n); for (let i = 0; i < n; i++) out[i] = f(i); return out }\n",
    );
}

#[test]
fn namespace_member_spread_call_invokes_const_rest_exports() {
    let dir = tempfile::tempdir().unwrap();
    lib(dir.path());
    write(
        dir.path(),
        "main.ts",
        "import * as Lib from \"./lib\"\n\
         import { fill, mk } from \"./lib\"\n\
         const other = fill(3, (i) => mk(\"k\".repeat(i + 1)))\n\
         console.log(Lib.count(...other), Lib.count(0, ...other), Lib.countFn(...other), Lib.countFn(0, ...other))\n\
         console.log(Lib.mergeAll(...other).mapUnsafe.size, (Lib as any)[\"mergeAll\"](...other).mapUnsafe.size)\n\
         const inner = (context: any) => Lib.mergeAll(...(context as any))\n\
         console.log(inner(other).mapUnsafe.size, Lib.mk(...([\"zz\"] as [string])).mapUnsafe.size)\n",
    );
    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "3 4 3 4\n3 3\n3 1\n"
    );
}
