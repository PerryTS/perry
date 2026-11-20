//! A `new` on a named or default import of a package that is not installed is
//! a compile error, like a call of one (#11576). Left to link, the construct
//! builds a plain object whose every member reads `undefined`, so the program
//! exits 0 over a package that is not there.
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
            "--no-auto-optimize",
            "--no-codegen",
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
        "import { Simple } from \"backend-not-installed-13667\";\n\
         const backend = new Simple();\n\
         console.log(typeof backend);\n",
    );
    assert!(!output.status.success(), "{message}");
    assert!(
        message.contains("Cannot find package 'backend-not-installed-13667'"),
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
        "import Client from \"client-not-installed-13667\";\n\
         export const client = new Client();\n",
    );
    assert!(!output.status.success(), "{message}");
    assert!(
        message.contains("Cannot find package 'client-not-installed-13667'"),
        "{message}"
    );
    assert!(message.contains("binding 'Client'"), "{message}");
}

#[test]
fn a_subpath_of_a_missing_package_names_the_package_not_the_subpath() {
    let (output, message, _dir) = compile(
        "import { Simple } from \"@scope-13667/pkg/backend/simple\";\n\
         const backend = new Simple();\n\
         console.log(typeof backend);\n",
    );
    assert!(!output.status.success(), "{message}");
    assert!(
        message.contains("Cannot find package '@scope-13667/pkg'"),
        "{message}"
    );
}

#[test]
fn a_class_shadowing_the_import_is_not_a_missing_package() {
    // `Expr::New` carries the class name alone, so this `new` on the local
    // `Simple` is indistinguishable from one on the import by name.
    let (output, message, _dir) = compile(
        "import { Simple } from \"backend-not-installed-13667\";\n\
         function make() {\n\
         \x20 class Simple {}\n\
         \x20 return new Simple();\n\
         }\n\
         console.log(typeof make());\n",
    );
    assert!(output.status.success(), "{message}");
}

#[test]
fn a_property_read_or_type_use_of_a_missing_package_keeps_compiling() {
    let (output, message, _dir) = compile(
        "import type { A } from \"types-not-installed-13667\";\n\
         import validator from \"validator-not-installed-13667\";\n\
         let a: A | undefined;\n\
         console.log(typeof a, typeof validator);\n",
    );
    assert!(output.status.success(), "{message}");
    assert!(!message.contains("Cannot find package"), "{message}");
}

#[test]
fn spread_construction_rejects_missing_named_default_and_subpath_imports() {
    for (declaration, source, package) in [
        ("{ Simple }", "missing-named-13547", "missing-named-13547"),
        ("Simple", "missing-default-13547", "missing-default-13547"),
        (
            "{ Simple }",
            "@missing-13547/pkg/subpath",
            "@missing-13547/pkg",
        ),
    ] {
        for arguments in ["'kind'", "...['kind']"] {
            let (output, message, dir) = compile(&format!(
                "import {declaration} from '{source}'; const value = new Simple({arguments}); console.log(value);"
            ));
            assert!(!output.status.success(), "{message}");
            assert!(
                message.contains(&format!("Cannot find package '{package}'")),
                "{message}"
            );
            assert!(message.contains("binding 'Simple'"), "{message}");
            assert!(message.contains("main.ts"), "{message}");
            assert!(!dir.path().join("objects").exists(), "{message}");
            assert!(
                std::fs::read_dir(dir.path()).unwrap().all(|entry| {
                    entry
                        .unwrap()
                        .path()
                        .extension()
                        .is_none_or(|extension| extension != "o")
                }),
                "a rejected construction must not emit an object"
            );
        }
    }
}
