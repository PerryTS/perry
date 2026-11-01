//! A `new` on a named import of a package that is not installed is a compile
//! error, like a call of one (#11576). Before this, construction was excluded
//! on the grounds that it "links, and throws where it is reached": it does
//! link, but nothing throws — the constructed value is a plain object whose
//! every member reads `undefined`, so the program ran to completion with
//! wrong results. Node fails the same program at load with
//! ERR_MODULE_NOT_FOUND.
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
fn constructing_an_import_of_a_missing_package_fails_with_the_package_name() {
    let (output, message, dir) = compile(
        "import { Simple } from \"backend-not-installed-13700\";\n\
         const backend = new Simple();\n\
         console.log(typeof backend);\n",
    );
    assert!(!output.status.success(), "{message}");
    assert!(
        message.contains("Cannot find package 'backend-not-installed-13700'"),
        "{message}"
    );
    assert!(message.contains("binding 'Simple'"), "{message}");
    assert!(message.contains("main.ts"), "{message}");
    assert!(
        !dir.path().join("objects").exists(),
        "a preflight failure must not produce objects"
    );
}

#[test]
fn constructing_a_default_import_of_a_missing_package_fails() {
    let (output, message, _dir) = compile(
        "import Client from \"client-not-installed-13700\";\n\
         export const client = new Client();\n",
    );
    assert!(!output.status.success(), "{message}");
    assert!(
        message.contains("Cannot find package 'client-not-installed-13700'"),
        "{message}"
    );
    assert!(message.contains("binding 'Client'"), "{message}");
}

#[test]
fn a_subpath_of_a_missing_package_names_the_package_not_the_subpath() {
    // The shape that found this: `@scope/pkg/backend/simple` resolves to
    // nothing, and the message must name the package to install.
    let (output, message, _dir) = compile(
        "import { Simple } from \"@scope-13700/pkg/backend/simple\";\n\
         const backend = new Simple();\n\
         console.log(typeof backend);\n",
    );
    assert!(!output.status.success(), "{message}");
    assert!(
        message.contains("Cannot find package '@scope-13700/pkg'"),
        "{message}"
    );
}

#[test]
fn a_property_read_or_type_use_of_a_missing_package_keeps_compiling() {
    // Unchanged by this fix: neither shape silently fabricates a value the
    // way `new` did, and #11576 deliberately keeps both non-fatal.
    let (output, message, _dir) = compile(
        "import type { A } from \"types-not-installed-13700\";\n\
         import validator from \"validator-not-installed-13700\";\n\
         let a: A | undefined;\n\
         console.log(typeof a, typeof validator);\n",
    );
    assert!(output.status.success(), "{message}");
    assert!(!message.contains("Cannot find package"), "{message}");
}
