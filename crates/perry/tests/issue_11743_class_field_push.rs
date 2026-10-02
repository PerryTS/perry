//! #11743: the original source spelling must reach the guarded append lane.
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDir(PathBuf);
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn compile(source: &str) -> (String, String) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = TestDir(std::env::temp_dir().join(format!(
        "perry-11743-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let entry = dir.0.join("main.ts");
    let binary = dir.0.join("main_bin");
    std::fs::write(&entry, source).unwrap();
    let build = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(&dir.0)
        .args([
            "compile",
            "--no-auto-optimize",
            "--no-cache",
            "--trace",
            "llvm",
        ])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let ir = std::fs::read_to_string(dir.0.join(".perry-trace/llvm/main_ts.ll")).unwrap();
    let run = Command::new(&binary).output().unwrap();
    assert!(
        run.status.success(),
        "execution failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    (ir, String::from_utf8(run.stdout).unwrap())
}

#[test]
fn original_recursive_class_field_push_emits_inline_append() {
    let (ir, stdout) = compile(
        r#"
class DocNode {
  children: DocNode[] = [];
  parent: DocNode | null;
  constructor(parent: DocNode | null) { this.parent = parent; }
}
function makeTree(depth: number, parent: DocNode | null): DocNode {
  const node = new DocNode(parent);
  if (depth > 0) {
    for (let i = 0; i < 4; i++) node.children.push(makeTree(depth - 1, node));
  }
  return node;
}
const root = makeTree(3, null);
console.log(root.children.length, root.children[3].parent === root);
"#,
    );
    assert_eq!(stdout, "4 true\n");
    let start = ir
        .lines()
        .position(|line| line.starts_with("define ") && line.contains("makeTree"))
        .unwrap();
    let body = ir
        .lines()
        .skip(start)
        .take_while(|line| *line != "}")
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        body.contains("fieldpush.header"),
        "missing receiver/method guard:\n{body}"
    );
    assert!(
        body.contains("apush.inbounds"),
        "missing inline append:\n{body}"
    );
    assert!(
        !body.contains("@js_typed_feedback_native_call_method_by_id"),
        "generic dispatch returned:\n{body}"
    );
}

#[test]
fn field_getters_own_methods_mutations_and_subclasses_keep_evaluation_order() {
    let (_, stdout) = compile(include_str!(
        "../../../test-files/test_gap_11743_class_field_array_push.ts"
    ));
    assert_eq!(
        stdout,
        concat!(
            "receiver 7 90\n",
            "getter field;arg; 8\n",
            "own snapshot arg;old:9:true; 0\n",
            "builtin snapshot  10\n",
            "method getter method;arg;call:11;\n",
            "subclass sub:12; 0\n",
            "wrong annotation object:13;\n",
            "frozen true\n",
            "recursive 35200\n",
        )
    );
}

#[test]
fn prototype_patches_keep_lookup_first_dispatch() {
    let (ir, stdout) = compile(include_str!(
        "../../../test-files/test_gap_11743_class_field_push_proto.ts"
    ));
    assert_eq!(
        stdout,
        "prototype snapshot arg;old:5:true; 0\ncustom prototype custom:6; 0\n"
    );
    assert!(
        !ir.contains("fieldpush.header"),
        "a patched prototype must keep method lookup"
    );
    assert!(
        ir.contains("@js_native_call_value"),
        "must call the captured method"
    );
}
