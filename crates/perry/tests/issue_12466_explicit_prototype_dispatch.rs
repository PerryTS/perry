//! A dynamic class tower must resolve the live inherited method on a shape miss.

use std::path::{Path, PathBuf};
use std::process::Command;

const GAP: &str = r#"
class Base {
  scale: number;
  constructor(n: number) { this.scale = n; }
  step(x: number) { return x * this.scale; }
}
class Derived extends Base { step(x: number) { return x * this.scale + 1; } }
function callAt(xs: any[], i: number) { return xs[i].step(3); }
const xs: Base[] = [new Base(2), new Derived(4)];
callAt(xs, 0);
Object.setPrototypeOf(xs[0], { step(x: number) { return this.scale + x + 99; } });
console.log(callAt(xs, 0));
"#;

const MUTATIONS: &str = r#"
class Base {
  scale: number;
  constructor(n: number) { this.scale = n; }
  step(x: number) { return x * this.scale + 1; }
}
class Derived extends Base { step(x: number) { return x * this.scale + 2; } }
class Third extends Base {}
class Fourth extends Derived { step(x: number) { return super.step(x) + 10; } }
function callAt(xs: Base[], i: number) { return xs[i].step(3); }
const xs: Base[] = [new Base(2), new Derived(4), new Third(6), new Fourth(8)];
console.log('birth', callAt(xs, 0), callAt(xs, 1), callAt(xs, 2), callAt(xs, 3));
(xs[2] as any).extra = 42;
(xs[3] as any).extra = 43;
console.log('data', callAt(xs, 2), callAt(xs, 3));
(xs[0] as any).step = function(x: number) { return this.scale + x + 100; };
console.log('own', callAt(xs, 0));
delete (xs[0] as any).step;
console.log('delete', callAt(xs, 0));
Derived.prototype.step = function(x: number) { return this.scale + x + 200; };
console.log('replace', callAt(xs, 1), callAt(xs, 3));
let gets = 0;
Object.defineProperty(Derived.prototype, 'step', {
  configurable: true, get() {
    gets++;
    return function(x: number) { return this.scale + x + 300; };
  }
});
console.log('getter', callAt(xs, 1), callAt(xs, 3), gets);
Object.defineProperty(Derived.prototype, 'step', {
  configurable: true, writable: true,
  value: function(x: number) { return this.scale + x + 350; }
});
console.log('define', callAt(xs, 1), callAt(xs, 3));
Object.setPrototypeOf(xs[0], {step(x: number) { return this.scale + x + 400; }});
console.log('edge', callAt(xs, 0), callAt(xs, 2));
Object.setPrototypeOf(xs[0], Base.prototype);
console.log('restore', callAt(xs, 0));
// Negative control: a plain object at the same site and an unrelated method.
console.log('plain', callAt([{step(x: number) { return x + 500; }}] as any, 0));
class Control { other(x: number) { return x + 1; } }
function control(o: Control) { return o.other(5); }
console.log('control', control(new Control()));
"#;

fn compile(dir: &Path, source: &str) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write source");
    let result = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir)
        .args([
            "compile",
            "--no-cache",
            "--no-auto-optimize",
            "--trace",
            "llvm",
        ])
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("compile fixture");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    output
}

fn run(binary: &Path) -> String {
    let result = Command::new(binary).output().expect("run fixture");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

fn read_ir(dir: &Path) -> String {
    let mut ir = String::new();
    for entry in std::fs::read_dir(dir).expect("read trace directory") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            ir.push_str(&read_ir(&path));
        } else if path.extension().is_some_and(|ext| ext == "ll") {
            ir.push_str(&std::fs::read_to_string(path).unwrap());
        }
    }
    ir
}

#[test]
fn instance_explicit_prototype_method_and_shape_miss_ir() {
    let dir = tempfile::tempdir().unwrap();
    let binary = compile(dir.path(), GAP);
    assert_eq!(run(&binary), "104\n");
    let ir = read_ir(&dir.path().join(".perry-trace/llvm"));
    // Check the actual fallback edge, not just a generic call elsewhere.
    let own_probe = ir.split("\n").collect::<Vec<_>>();
    let probe = own_probe
        .iter()
        .position(|line| line.contains("call double @js_object_get_own_field_or_undef("))
        .expect("fixture must exercise the tower's own probe");
    let branch = own_probe[probe..]
        .iter()
        .find(|line| line.contains("br i1 "))
        .unwrap();
    assert!(
        branch.contains("label %idispatch.default"),
        "shape miss must use the generic operation: {branch}"
    );
    assert!(
        !branch.contains("label %idisp.dispatch"),
        "shape miss revived the nominal class method"
    );
    assert!(
        ir.contains("call double @js_native_call_method_by_id("),
        "generic operation must remain emitted"
    );
    assert!(
        ir.lines()
            .any(|line| line.contains("br i1 ") && line.contains("label %idisp.dispatch")),
        "canonical shape hit must still enter the existing tower"
    );
}

#[test]
fn class_dispatch_mutations_inheritance_and_negative_controls() {
    let dir = tempfile::tempdir().unwrap();
    let binary = compile(dir.path(), MUTATIONS);
    assert_eq!(run(&binary),
        "birth 7 14 19 36\ndata 19 36\nown 105\ndelete 7\nreplace 207 221\ngetter 307 321 2\ndefine 357 371\nedge 405 19\nrestore 7\nplain 503\ncontrol 6\n");
}
