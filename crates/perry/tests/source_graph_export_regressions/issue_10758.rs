use super::{compile_and_run, write};

#[test]
fn esm_module_resolves_static_json_require_from_its_own_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package = dir.path().join("mongodb");
    let handshake = package.join("src/cmap/handshake");
    std::fs::create_dir_all(&handshake).expect("create package source tree");
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"mongodb","version":"7.5.0"}"#,
    )
    .expect("write package.json");
    std::fs::write(handshake.join("marker.ts"), "export const marker = '';\n")
        .expect("write marker module");
    std::fs::write(
        handshake.join("client_metadata.ts"),
        "import { marker } from './marker';\n\
         export const driverVersion = require('../../../package.json').version + marker;\n",
    )
    .expect("write client metadata module");
    write(
        dir.path(),
        "main.ts",
        "import { driverVersion } from './mongodb/src/cmap/handshake/client_metadata';\n\
         console.log(driverVersion);\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "7.5.0\n");
}
