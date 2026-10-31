// A CommonJS module's namespace holds exactly what the module puts on
// `exports`/`module.exports` (plus `default` and `module.exports`), never its
// non-exported top-level classes, functions or consts.
import * as ns from "./gap_cjs_namespace_exports_helper.cjs";
import lib from "./gap_cjs_namespace_exports_helper.cjs";
import * as solo from "./gap_cjs_namespace_exports_default_helper.cjs";
import Solo from "./gap_cjs_namespace_exports_default_helper.cjs";
import { Pub, Alias } from "./gap_cjs_namespace_exports_helper.cjs";

const nsAny: any = ns;
console.log("keys", Object.keys(ns).sort().join(","));
for (const name of ["Hidden", "Base", "Pub", "Alias", "Boxed", "helperFn", "helperConst", "box", "run"]) {
    console.log(name, name in ns, typeof nsAny[name], name in lib);
}
console.log("default keys", Object.keys(lib).sort().join(","));
console.log("pub", new nsAny.Pub().pub(), new nsAny.Pub().base(), nsAny.Pub === lib.Pub);
console.log("alias", new nsAny.Alias().base(), nsAny.Alias === lib.Alias);
console.log("boxed", typeof nsAny.Boxed, typeof nsAny.Boxed.inner, new nsAny.Boxed.inner().boxed(), nsAny.Boxed === lib.Boxed);
console.log("run", nsAny.run());
console.log("solo keys", Object.keys(solo).sort().join(","));
const soloAny: any = solo;
console.log("solo", "Solo" in solo, typeof soloAny.Solo, soloAny.default === Solo, new Solo().solo());

// Class identity of the exported classes survives (subclassing, super calls).
class SubSolo extends Solo {
    sub() { return super.solo() + "!"; }
}
class SubPub extends Pub {
    sub() { return super.pub() + "/" + super.base() + "!"; }
}
console.log("sub", new SubSolo().sub(), new SubSolo() instanceof Solo, new SubPub().sub(), new SubPub() instanceof Alias);
