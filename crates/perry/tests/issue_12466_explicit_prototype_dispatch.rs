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

const GAPS: &str = r#"
class Base {
  scale: number;
  constructor(n: number) { this.scale = n; }
  step(x: number) { return this.scale * x; }
}
class Sub extends Base { step(x: number) { return super.step(x) + 100; } }
class Other { step(x: number) { return x + 500; } }
function callAt(xs: any[], i: number) { return xs[i].step(3); }
const c = new Base(2);
(c as any).extra = 42;
const xs: any[] = [c, new Other()];
console.log('first', callAt(xs, 0));
// Prove the foreign subclass edge while both declaration holders are pristine.
Object.setPrototypeOf(c, Sub.prototype);
console.log('subclass-edge', callAt(xs, 0));
Object.setPrototypeOf(c, Base.prototype);
console.log('restored', callAt(xs, 0));
// Same prototype object, new method value AFTER the site has run.
Base.prototype.step = function(x: number) { return this.scale + x + 20; };
console.log('in-place', callAt(xs, 0));
// A foreign intermediary with an own override must outrank Base.prototype.
const intermediate = Object.create(Base.prototype);
intermediate.step = function(x: number) { return this.scale + x + 200; };
Object.setPrototypeOf(c, intermediate);
console.log('created-edge-own', callAt(xs, 0));
// Also exercise an Object.create receiver with its own method.
const created = Object.create(Base.prototype);
created.scale = 7;
created.step = function(x: number) { return this.scale + x + 300; };
console.log('created-own', callAt([created], 0));
// An ordinary subclass birth with extra fields remains correct too.
const sub = new Sub(4);
(sub as any).extra = 43;
console.log('subclass-birth', callAt([sub], 0));
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
    // The program relinks a prototype (`Object.setPrototypeOf`), so the
    // prototype-write census proves no method constant (object model step 3):
    // the class tower keeps no compiled case and every receiver, the
    // re-parented one included, takes the generic method site.
    let call_at = ir
        .split("\ndefine ")
        .find(|f| f.contains("callAt") && f.contains("idispatch.generic"))
        .expect("fixture must emit the class-dispatch site");
    assert!(
        !call_at.contains("pconst.case"),
        "a relinking program has no prototype constants: {call_at}"
    );
    assert!(
        !call_at.contains("certify") && !call_at.contains("@PERRY_PROTO_VALIDITY"),
        "no runtime proof remains: {call_at}"
    );
    assert!(
        !call_at.contains("@js_object_get_own_field_or_undef("),
        "the site must not probe own properties"
    );
}

#[test]
fn class_dispatch_mutations_inheritance_and_negative_controls() {
    let dir = tempfile::tempdir().unwrap();
    let binary = compile(dir.path(), MUTATIONS);
    assert_eq!(run(&binary),
        "birth 7 14 19 36\ndata 19 36\nown 105\ndelete 7\nreplace 207 221\ngetter 307 321 2\ndefine 357 371\nedge 405 19\nrestore 7\nplain 503\ncontrol 6\n");
}

#[test]
fn prototype_replacement_after_first_call_subclass_edge_and_created_override() {
    let dir = tempfile::tempdir().unwrap();
    let binary = compile(dir.path(), GAPS);
    assert_eq!(run(&binary), "first 6\nsubclass-edge 106\nrestored 6\nin-place 25\ncreated-edge-own 205\ncreated-own 310\nsubclass-birth 127\n");
}
