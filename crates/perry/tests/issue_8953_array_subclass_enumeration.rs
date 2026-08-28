//! #8953: Array-subclass instances use an elements store for indices and
//! `length`, while inherited Array methods stay off the instance shape.
use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.js");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    Command::new(&output).output().expect("run compiled binary")
}

#[test]
fn array_subclass_enumeration_matches_node_and_fill_is_inherited() {
    let run = compile_and_run(
        r#"
class A extends Array {}

const empty = new A();
console.log("empty keys:", Object.keys(empty).join(","));
let emptyForIn = [];
for (const key in empty) emptyForIn.push(key);
console.log("empty for-in:", emptyForIn.join(","));
console.log("empty names:", Object.getOwnPropertyNames(empty).join(","));
console.log("fill:", Object.hasOwn(empty, "fill"), typeof empty.fill);

const a = new A();
a.push(1, 2, 3);
console.log("keys:", Object.keys(a).join(","));
let keys = [];
for (const key in a) keys.push(key);
console.log("for-in:", keys.join(","));
console.log("names:", Object.getOwnPropertyNames(a).join(","));
a.fill(7, 1);
console.log("filled:", a.join(","));
const inheritedFill = a.fill;
inheritedFill.call(a, 9, 2);
console.log("extracted fill:", a.join(","));

class B extends Array { fill(value) { return "override:" + value; } }
const b = new B();
console.log("override:", Object.hasOwn(b, "fill"), b.fill(5));
"#,
    );
    assert!(
        run.status.success(),
        "the #8953 fixture must not crash\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "empty keys: \n\
empty for-in: \n\
empty names: length\n\
fill: false function\n\
keys: 0,1,2\n\
for-in: 0,1,2\n\
names: 0,1,2,length\n\
filled: 1,7,7\n\
extracted fill: 1,7,9\n\
override: false override:5\n"
    );
}
