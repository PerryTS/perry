//! #11576: a value-used named/default import of a bare package that is not
//! installed is a compile error naming the package and the binding, instead of
//! a linker error on the local binding name (`_nanoid`, `_uuidv4`, `_fastify`).
use std::path::Path;
use std::process::{Command, Output};

fn compile(main: &str) -> (Output, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root: &Path = dir.path();
    std::fs::write(root.join("package.json"), r#"{"type":"module"}"#).unwrap();
    std::fs::write(root.join("main.ts"), main).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(root)
        .args([
            "compile",
            "main.ts",
            "--no-cache",
            "--no-link",
            "-o",
            "objects",
        ])
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .unwrap();
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output, message, dir)
}

#[test]
fn value_used_imports_of_missing_packages_fail_with_the_package_name() {
    let (output, message, dir) = compile(
        "import { v4 as uuidv4 } from \"uuid-not-installed-11576\";\n\
         import nanoid from \"nanoid-not-installed-11576\";\n\
         console.log(uuidv4(), nanoid());\n",
    );
    assert!(!output.status.success(), "{message}");
    assert!(
        message.contains("Cannot find package 'nanoid-not-installed-11576'"),
        "{message}"
    );
    assert!(message.contains("binding 'nanoid'"), "{message}");
    assert!(message.contains("main.ts"), "{message}");
    assert!(
        message.contains("Also not installed: uuid-not-installed-11576."),
        "{message}"
    );
    assert!(
        !dir.path().join("objects").exists(),
        "a preflight failure must not produce objects"
    );
}

#[test]
fn type_only_and_property_uses_of_missing_packages_keep_compiling() {
    // Only a direct call reached the linker as an undefined symbol; a type use
    // or a property read on the binding linked before and still does (the
    // unresolved-import warning is unchanged for them).
    let (output, message, _dir) = compile(
        "import type { A } from \"types-not-installed-11576\";\n\
         import { B } from \"also-types-not-installed-11576\";\n\
         import validator from \"validator-not-installed-11576\";\n\
         let a: A | B | undefined;\n\
         console.log(typeof a, typeof validator);\n",
    );
    assert!(output.status.success(), "{message}");
    assert!(!message.contains("Cannot find package"), "{message}");
}
