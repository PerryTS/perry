use super::{compile_and_run, write};

#[test]
fn named_node_builtin_exports_survive_local_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "direct.ts",
        "export { createHash } from 'node:crypto';\n",
    );
    write(
        dir.path(),
        "local.ts",
        "import { join } from 'node:path';\n\
         export { join as pathJoin };\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { createHash } from './direct';\n\
         import { pathJoin } from './local';\n\
         console.log(typeof createHash, typeof pathJoin);\n\
         console.log(createHash('sha256').update('x').digest('hex'));\n\
         console.log(pathJoin('a', 'b'));\n",
    );

    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "function function\n\
         2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881\n\
         a/b\n"
    );
}
