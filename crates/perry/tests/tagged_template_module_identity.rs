//! Sibling and linked dependency modules need distinct identities during
//! lowering, before discovery recomputes the common project root.
use std::process::Command;

#[test]
fn tagged_template_module_identity_keeps_same_named_external_modules_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tag.ts");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        r#"export function tag(s: TemplateStringsArray, ...values: any[]) {
 return JSON.stringify(s) + ':' + JSON.stringify(s.raw) + ':' + values.map(String).join('|');
}
"#,
    )
    .unwrap();
    let path = dir.path().join("a/index.ts");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        r#"import {tag} from '../tag.ts';
export function first(x: any) {return tag`a\n${x}end`;}
"#,
    )
    .unwrap();
    let path = dir.path().join("b/index.ts");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        r#"import {tag} from '../tag.ts';
export function second(x: any) {return tag`b\t${x}done`;}
"#,
    )
    .unwrap();
    let path = dir.path().join("app/main.ts");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        r#"import {first} from '../a/index.ts';
import {second} from '../b/index.ts';
const o = {toString() {return 'object';}};
console.log(first(o));
console.log(second(o));
console.log(first(second(42)));
"#,
    )
    .unwrap();
    let app = dir.path().join("app");
    let binary = dir.path().join("program");
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(&app)
        .args([
            "compile",
            "--no-auto-optimize",
            "--no-cache",
            "main.ts",
            "-o",
        ])
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(binary).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    // Node's cooked/raw pairs, with object coercion and a nested template.
    assert_eq!(
        String::from_utf8(run.stdout).unwrap(),
        concat!(
            r#"["a\n","end"]:["a\\n","end"]:object"#,
            "\n",
            r#"["b\t","done"]:["b\\t","done"]:object"#,
            "\n",
            r#"["a\n","end"]:["a\\n","end"]:["b\t","done"]:["b\\t","done"]:42"#,
            "\n",
        )
    );
}
