//! Regression test for #9940: an Error subclass declared in a
//! `compilePackages` dependency must share the application's global Error
//! identity. Frameworks such as Hono use `value instanceof Error` to decide
//! whether a thrown value reaches their error handler.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn compiled_package_error_subclass_is_instanceof_global_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "compile-package-error-identity",
  "private": true,
  "type": "module",
  "perry": {
    "compilePackages": ["error-package"],
    "allow": { "compilePackages": ["error-package"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let package = root.join("node_modules").join("error-package");
    std::fs::create_dir_all(package.join("src")).expect("mkdir error-package/src");
    std::fs::write(
        package.join("package.json"),
        r#"{
  "name": "error-package",
  "version": "1.0.0",
  "type": "module",
  "exports": { ".": "./index.js" }
}"#,
    )
    .expect("write error-package package.json");
    std::fs::write(
        package.join("index.js"),
        "export { PackageError, makeError } from \"./src/errors.js\";\n",
    )
    .expect("write error-package published entry");
    std::fs::write(
        package.join("src/core.ts"),
        r#"export function constructorFactory(name: string, params?: { Parent?: any }): any {
  const Parent = params?.Parent ?? Object;
  class Definition extends Parent {}
  function DynamicPackageError(message: string) {
    const inst: any = params?.Parent ? new Definition() : this;
    inst.message = message;
    inst.kind = name;
    return inst;
  }
  Object.defineProperty(DynamicPackageError, Symbol.hasInstance, {
    value: (inst: any) => inst?.kind === name,
  });
  Object.defineProperty(DynamicPackageError, "name", { value: name });
  return DynamicPackageError;
}
"#,
    )
    .expect("write error-package core.ts");
    std::fs::write(
        package.join("src/errors.ts"),
        r#"import { constructorFactory } from "./core.js";

// Zod creates many Object-backed classes from this factory before it creates
// its Error-backed class from the same nested class declaration.
export const PlainThing = constructorFactory("PlainThing");
export const PackageError = constructorFactory("PackageError", { Parent: Error });
export const AnotherPlainThing = constructorFactory("AnotherPlainThing");
export function makeError(message: string): any {
  return new PackageError(message);
}
"#,
    )
    .expect("write error-package errors.ts");
    std::fs::write(
        package.join("src/index.ts"),
        "export { PackageError, makeError } from \"./errors.js\";\n",
    )
    .expect("write error-package src/index.ts");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import { makeError } from "error-package";

const packageError: any = makeError("bad input");
class AppError extends Error {}
console.log(
  packageError instanceof Error,
  new AppError("app") instanceof Error,
  new Error("plain") instanceof Error
);
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        stdout, "true true true\n",
        "the package subclass must inherit the application's global Error identity"
    );
}
