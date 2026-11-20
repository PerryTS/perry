//! A TypeScript import specifier that names only a type binds nothing, so a
//! local declaration of the same name is no redeclaration: tsc elides the
//! specifier and the declaration merges with it. TypeScript's own
//! `utilities.ts` imports the `Symbol`, `Type`, `Signature` and `Node`
//! interfaces from its namespace barrel and declares `function Symbol` and the
//! rest beside them. A specifier whose export is a value still binds, and the
//! local declaration is the early SyntaxError.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn compile(root: &Path) -> (Output, PathBuf) {
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_path_buf());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let executable = root.join(if cfg!(windows) {
        "app.exe"
    } else {
        "app-native"
    });
    let output = Command::new(compiler)
        .current_dir(root)
        .env("PERRY_RUNTIME_DIR", runtime)
        .env("PERRY_WORKSPACE_ROOT", workspace)
        .args(["compile", "--no-cache", "--no-auto-optimize"])
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("run perry compile");
    (output, executable)
}

fn write(root: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        std::fs::write(root.join(name), text).unwrap();
    }
}

#[test]
fn local_declaration_merges_with_an_imported_type() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(
        &root,
        &[
            (
                "types.ts",
                "export interface Symbol { flags: number; name: string; }\n\
                 export type Node = { kind: number };\n",
            ),
            ("ns.ts", "export * from \"./types.js\";\n"),
            (
                "utilities.ts",
                "import { Symbol, Node } from \"./ns.js\";\n\
                 function Symbol(this: Symbol, flags: number, name: string) {\n\
                 \x20 this.flags = flags;\n\
                 \x20 this.name = name;\n\
                 }\n\
                 function Node(this: Node, kind: number) {\n\
                 \x20 this.kind = kind;\n\
                 }\n\
                 export function make(flags: number, name: string): Symbol {\n\
                 \x20 return new (Symbol as any)(flags, name);\n\
                 }\n\
                 export function node(kind: number): Node {\n\
                 \x20 return new (Node as any)(kind);\n\
                 }\n",
            ),
            (
                "main.ts",
                "import { make, node } from \"./utilities.js\";\n\
                 const s = make(4, \"x\");\n\
                 console.log(s.flags, s.name, typeof s, node(7).kind);\n",
            ),
        ],
    );
    let (compile, executable) = compile(&root);
    assert!(
        compile.status.success(),
        "an imported interface or type alias binds nothing\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&executable)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(run.status.success(), "{:?}", run);
    // node on tsc's emit prints the same line.
    assert_eq!(String::from_utf8_lossy(&run.stdout), "4 x object 7\n");
}

#[test]
fn local_declaration_repeating_an_imported_value_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(
        &root,
        &[
            (
                "lib.ts",
                "export interface helper { n: number; }\n\
                 export function helper(): string { return \"lib\"; }\n",
            ),
            ("ns.ts", "export * from \"./lib.js\";\n"),
            (
                "main.ts",
                "import { helper } from \"./ns.js\";\n\
                 function helper(): string { return \"local\"; }\n\
                 console.log(helper());\n",
            ),
        ],
    );
    let (compile, _) = compile(&root);
    assert!(
        !compile.status.success(),
        "an imported function binds its name"
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(
        stderr.contains("SyntaxError: Identifier 'helper' has already been declared"),
        "stderr:\n{stderr}"
    );
}
