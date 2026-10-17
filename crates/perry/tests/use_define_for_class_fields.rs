//! `useDefineForClassFields` (owner decision 2026-10-17): a TypeScript module
//! follows its tsconfig. Under `false` (the default before ES2022) a field
//! without an initializer creates no property and an initializer is a [[Set]]
//! in the constructor; under `true` fields are [[Define]]d. Expected output is
//! TypeScript 5.8 (`tsc --target ES2020|ES2022`) run by node.

use std::path::PathBuf;
use std::process::Command;

fn compile_and_run(source: &str, tsconfig: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    std::fs::write(dir.path().join("tsconfig.json"), tsconfig).expect("write tsconfig");
    let perry = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime_dir = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| perry.parent().expect("binary directory").to_path_buf());
    let compile = Command::new(&perry)
        .current_dir(dir.path())
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir)
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "run failed ({:?})\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("UTF-8 stdout")
}

const FIELDS: &str = r#"
const log: string[] = [];
const own = (o: object) => Object.keys(o).join(",");
const has = (o: object, k: string) => Object.prototype.hasOwnProperty.call(o, k);

class Base {
  set tagged(v: unknown) { log.push("Base.tagged setter " + String(v)); }
  get tagged(): unknown { return "getter"; }
}
class Fields extends Base {
  plain: number;
  definite!: string;
  optional?: boolean;
  declare declared: number;
  tagged: unknown = 5;
  first = 1;
  second = this.first + 1;
  arrow = () => this.first;
  #hidden = 3;
  static staticBare: number;
  static staticInit = 9;
  param: number;
  constructor(param: number) {
    super();
    this.param = param;
    log.push("ctor sees first=" + this.first + " param=" + this.param);
    this.plain = 2;
  }
  hidden() { return this.#hidden; }
}
const f = new Fields(7);
console.log(own(f));
console.log(has(f, "definite"), has(f, "optional"), has(f, "declared"), has(f, "tagged"));
console.log(f.second, f.arrow(), f.hidden(), String(f.tagged));
console.log(has(Fields, "staticBare"), has(Fields, "staticInit"), Fields.staticInit);

class NoCtor extends Fields {
  extra = "x";
  bare!: string;
}
const n = new NoCtor(4);
console.log(own(n), has(n, "bare"));

class Plain {
  a = 1;
  b: number;
  c!: string;
}
const p = new Plain();
console.log(own(p), has(p, "b"), has(p, "c"));
p.c = "set";
console.log(own(p));

class Clone {
  fileName!: string;
  text!: string;
  kind: number;
  constructor(kind: number) { this.kind = kind; }
}
const src: any = new Clone(1);
src.fileName = "a.ts";
src.text = "t";
const dst: any = new Clone(1);
for (const k in src) {
  if (has(dst, k) || !has(src, k)) continue;
  dst[k] = src[k];
}
console.log(dst.fileName, dst.text, own(dst));
console.log(log.join(" | "));
"#;

const ASSIGNED: &str = "first,second,arrow,param,plain
false false false false
2 1 3 getter
false true 9
first,second,arrow,param,plain,extra false
a false false
a,c
a.ts t kind,fileName,text
Base.tagged setter 5 | ctor sees first=1 param=7 | Base.tagged setter 5 | ctor sees first=1 param=4
";

const DEFINED: &str = "plain,definite,optional,tagged,first,second,arrow,param
true true false true
2 1 3 5
true true 9
plain,definite,optional,tagged,first,second,arrow,param,extra,bare true
a,b,c true true
a,b,c
undefined undefined fileName,text,kind
ctor sees first=1 param=7 | ctor sees first=1 param=4
";

#[test]
fn target_before_es2022_assigns_fields() {
    let out = compile_and_run(FIELDS, r#"{"compilerOptions":{"target":"ES2020"}}"#);
    assert_eq!(out, ASSIGNED);
}

#[test]
fn explicit_false_overrides_target() {
    let config = r#"{"compilerOptions":{"target":"ESNext","useDefineForClassFields":false}}"#;
    assert_eq!(compile_and_run(FIELDS, config), ASSIGNED);
}

#[test]
fn es2022_target_defines_fields() {
    let out = compile_and_run(FIELDS, r#"{"compilerOptions":{"target":"ES2022"}}"#);
    assert_eq!(out, DEFINED);
}

#[test]
fn explicit_true_overrides_target() {
    let config = r#"{"compilerOptions":{"target":"es5","useDefineForClassFields":true}}"#;
    assert_eq!(compile_and_run(FIELDS, config), DEFINED);
}

#[test]
fn no_target_defines_fields() {
    let out = compile_and_run(FIELDS, r#"{"compilerOptions":{"strict":true}}"#);
    assert_eq!(out, DEFINED);
}

/// Parameter properties are stored first, then the initializers.
#[test]
fn assigned_initializers_follow_parameter_properties() {
    let source = r#"
class Props {
  late = "late";
  constructor(public early: string) {}
}
class Derived extends Props {
  more = this.early + "!";
  constructor(public extra: number) { super("e"); }
}
console.log(Object.keys(new Props("early")).join(","));
const d = new Derived(1);
console.log(Object.keys(d).join(","), d.more);
"#;
    let out = compile_and_run(source, r#"{"compilerOptions":{"target":"ES2020"}}"#);
    assert_eq!(out, "early,late\nearly,late,extra,more e!\n");
}
