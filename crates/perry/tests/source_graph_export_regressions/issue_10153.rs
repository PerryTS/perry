//! Babel's CommonJS accessor re-exports must remain live value bindings.

use super::{compile_and_run, write};

fn fixtures(dir: &std::path::Path) {
    write(
        dir,
        "index.cjs",
        include_str!("../../../../test-files/cjs_getter_reexport/index.cjs"),
    );
    write(
        dir,
        "transform.cjs",
        include_str!("../../../../test-files/cjs_getter_reexport/transform.cjs"),
    );
}

#[test]
fn named_cjs_getter_reexport_is_a_live_value() {
    let dir = tempfile::tempdir().unwrap();
    fixtures(dir.path());
    write(
        dir.path(),
        "main.mjs",
        include_str!("../../../../test-files/test_cjs_getter_reexport.ts")
            .replace("./cjs_getter_reexport/index.cjs", "./index.cjs")
            .as_str(),
    );
    assert_eq!(
        compile_and_run(dir.path(), "main.mjs"),
        "0\nfunction\n2\n3\nundefined\n12\n13\n14\n3\n"
    );
}

#[test]
fn namespace_cjs_getter_reexport_is_a_live_value() {
    let dir = tempfile::tempdir().unwrap();
    fixtures(dir.path());
    write(
        dir.path(),
        "main.mjs",
        "import * as ns from './index.cjs';\n\
         console.log(ns.reads());\n\
         console.log(typeof ns.transform);\n\
         console.log(ns.transform(1));\n\
         console.log(ns.value(2));\n\
         console.log(typeof ns.late);\n\
         ns.update();\n\
         console.log(ns.transform(1));\n\
         console.log(ns.value(2));\n\
         console.log(ns.late(3));\n\
         console.log(ns.reads());\n",
    );
    assert_eq!(
        compile_and_run(dir.path(), "main.mjs"),
        "0\nfunction\n2\n3\nundefined\n12\n13\n14\n3\n"
    );
}

#[test]
fn cjs_getter_reexport_survives_barrels_and_materialized_namespaces() {
    let dir = tempfile::tempdir().unwrap();
    fixtures(dir.path());
    write(
        dir.path(),
        "barrel.mjs",
        "export { transform as renamed, update, reads } from './index.cjs';\n",
    );
    write(
        dir.path(),
        "main.mjs",
        "import { renamed as transform, update, reads } from './barrel.mjs';\n\
         import * as ns from './barrel.mjs';\n\
         const materialized = ns;\n\
         console.log(reads());\n\
         console.log(transform(1));\n\
         console.log(materialized.renamed(2));\n\
         update();\n\
         console.log(transform(1));\n\
         console.log(materialized.renamed(2));\n\
         console.log(reads());\n",
    );
    assert_eq!(
        compile_and_run(dir.path(), "main.mjs"),
        "0\n2\n3\n12\n13\n4\n"
    );
}

#[test]
fn dynamic_cjs_namespace_keeps_getter_reexports_live() {
    let dir = tempfile::tempdir().unwrap();
    fixtures(dir.path());
    write(
        dir.path(),
        "main.mjs",
        "const ns = await import('./index.cjs');\n\
         const key = process.argv[2] || 'transform';\n\
         console.log(ns.reads());\n\
         console.log(Reflect.get(ns, key)(1));\n\
         ns.update();\n\
         console.log(Reflect.get(ns, key)(1));\n\
         console.log(ns.reads());\n",
    );
    assert_eq!(compile_and_run(dir.path(), "main.mjs"), "0\n2\n12\n2\n");
}

#[test]
fn ordinary_esm_property_export_keeps_its_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "value.mjs",
        "const _cjs = { value: 1 };\n\
         export const value = _cjs.value;\n\
         export function update() { _cjs.value = 2; }\n",
    );
    write(
        dir.path(),
        "main.mjs",
        "import { value, update } from './value.mjs';\n\
         console.log(value);\n\
         update();\n\
         console.log(value);\n",
    );
    assert_eq!(compile_and_run(dir.path(), "main.mjs"), "1\n1\n");
}
