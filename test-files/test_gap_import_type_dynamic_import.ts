// An `import type` of a module must not drop a real `import()` of the same
// module in the same file. The type import is erased; the dynamic import must
// still load the module, which is compiled into the binary.
import type { Shape } from "./_helpers/import_type_dynamic_target.ts";
import { type Tagged } from "./_helpers/import_type_dynamic_reexport.ts";

let loading: Promise<Shape | undefined> | undefined;
const load = () =>
    (loading ??= import("./_helpers/import_type_dynamic_target.ts").then(
        (m) => {
            console.log("target name:", m.name);
            return m.make();
        },
        (e: Error) => {
            console.log("target import rejected:", e.message);
            return undefined;
        },
    ));

const shape = await load();
console.log("sides:", shape?.sides);
console.log("same promise:", load() === load());

try {
    const m = await import("./_helpers/import_type_dynamic_reexport.ts");
    console.log("reexport keys:", Object.keys(m).sort().join(","));
    const tagged: Tagged = { tag: m.own + "+" + m.innerFn() };
    console.log("tag:", tagged.tag);
} catch (e: any) {
    console.log("reexport import rejected:", e.message);
}
