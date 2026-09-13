//! `export * as Self from "./self"` must read as the module's own namespace
//! through a dynamic `import()` namespace, not as `undefined` (#10160).

use super::{compile_and_run, write};

fn store(dir: &std::path::Path) {
    write(
        dir,
        "store.ts",
        "export class Service {\n\
         \x20 static use(f: (s: string) => string) { return f(\"store-ok\") }\n\
         }\n\
         export const node = \"node-layer\"\n\
         export * as InstanceStore from \"./store\"\n",
    );
}

#[test]
fn self_namespace_reexport_is_defined_on_a_dynamic_import_namespace() {
    let dir = tempfile::tempdir().unwrap();
    store(dir.path());
    write(
        dir.path(),
        "main.ts",
        "import { InstanceStore as Static } from \"./store\"\n\
         console.log(typeof Static, typeof Static.Service, Static.Service.use((s) => s))\n\
         const mod = await import(\"./store\")\n\
         console.log(Object.keys(mod).sort().join(\",\"))\n\
         const { InstanceStore } = mod\n\
         console.log(typeof InstanceStore, typeof InstanceStore.Service, InstanceStore.Service.use((s) => s + \"!\"))\n\
         console.log(InstanceStore.InstanceStore === InstanceStore, InstanceStore.node, Static.node)\n",
    );
    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "object function store-ok\n\
         InstanceStore,Service,node\n\
         object function store-ok!\n\
         true node-layer node-layer\n"
    );
}

#[test]
fn self_namespace_reexport_is_defined_on_a_static_star_import() {
    let dir = tempfile::tempdir().unwrap();
    store(dir.path());
    write(
        dir.path(),
        "main.ts",
        "import * as ns from \"./store\"\n\
         console.log(typeof ns.InstanceStore, ns.InstanceStore.Service.use((s) => s), ns.InstanceStore.node)\n",
    );
    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "object store-ok node-layer\n"
    );
}

#[test]
fn foreign_namespace_reexport_stays_intact_on_a_dynamic_import_namespace() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "other.ts", "export const value = 42\n");
    write(
        dir.path(),
        "barrel.ts",
        "export * as Other from \"./other\"\nexport * as Barrel from \"./barrel\"\n",
    );
    write(
        dir.path(),
        "main.ts",
        "const mod = await import(\"./barrel\")\n\
         console.log(typeof mod.Other, mod.Other.value, typeof mod.Barrel, mod.Barrel.Other.value)\n",
    );
    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "object 42 object 42\n"
    );
}
