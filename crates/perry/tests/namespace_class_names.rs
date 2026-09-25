//! Imported class names belong to the defining module, including namespace reexports.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn successful(command: &mut Command, subject: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{subject}: {error}"));
    assert!(
        output.status.success(),
        "{subject} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn namespace_class_names_match_node() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(
        root.join("binary.ts"),
        r#"export class Binary {}
export class UUID extends Binary {}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("bson.ts"),
        r#"export { Binary, UUID } from "./binary.ts";
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("index.ts"),
        r#"import * as BSON from "./bson.ts";
export * from "./bson.ts";
export { BSON };
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("control.ts"),
        r#"export * as Control from "./binary.ts";
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("alias.ts"),
        r#"export { UUID as PublicUUID, Binary as PublicBinary } from "./binary.ts";
"#,
    )
    .unwrap();
    std::fs::write(root.join("main.ts"), r#"import { UUID, Binary, BSON } from './index.ts';
import { PublicUUID as Renamed, PublicBinary } from './alias.ts';
import { Control } from './control.ts';
console.log('named', UUID.name, Binary.name);
console.log('namespace', BSON.UUID.name, BSON.Binary.name);
console.log('instance', new UUID().constructor.name, new BSON.UUID().constructor.name);
console.log('alias', Renamed.name, PublicBinary.name, new Renamed().constructor.name);
console.log('control', Control.UUID.name, Control.Binary.name);
console.log('identity', BSON.UUID === UUID, Renamed === UUID, Control.UUID === UUID);
console.log('inheritance', new Renamed() instanceof Binary, new BSON.UUID() instanceof PublicBinary);
"#).unwrap();
    std::fs::write(
        root.join("control-main.ts"),
        r#"import { Control } from './control.ts';
console.log(Control.UUID.name, Control.Binary.name, new Control.UUID().constructor.name);
console.log(new Control.UUID() instanceof Control.Binary);
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("original-main.ts"),
        r#"import { UUID, Binary, BSON } from './index.ts';
console.log(JSON.stringify(UUID.name));
console.log(JSON.stringify(new UUID().constructor.name));
console.log(JSON.stringify(Binary.name));
console.log(JSON.stringify(BSON.UUID.name));
console.log(BSON.UUID === UUID);
"#,
    )
    .unwrap();
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_path_buf());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    // The standalone control has no leaking consumer initializer in its graph.
    for (name, expected) in [
        ("original-main.ts", "\"UUID\"\n\"UUID\"\n\"Binary\"\n\"UUID\"\ntrue\n"),
        ("main.ts", "named UUID Binary\nnamespace UUID Binary\ninstance UUID UUID\nalias UUID Binary UUID\ncontrol UUID Binary\nidentity true true true\ninheritance true true\n"),
        ("control-main.ts", "UUID Binary UUID\ntrue\n"),
    ] {
        let entry = root.join(name);
        let node = successful(Command::new("node").current_dir(&root).arg(&entry), name);
        assert_eq!(node.stdout, expected.as_bytes(), "Node fixture: {name}");
        let executable = root.join(if cfg!(windows) { "app.exe" } else { "app-native" });
        successful(
            Command::new(&compiler)
                .current_dir(&root)
                .env("PERRY_RUNTIME_DIR", &runtime)
                .env("PERRY_WORKSPACE_ROOT", &workspace)
                .args(["compile", "--no-cache", "--no-auto-optimize"])
                .arg(&entry).arg("-o").arg(&executable),
            name,
        );
        let native = successful(Command::new(&executable).current_dir(&root), name);
        assert_eq!(native.stdout, node.stdout, "class names and identity: {name}");
    }
}
