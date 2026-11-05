//! A straight-line run of 2c accesses of one receiver proves the
//! receiver's layout record once (its head); each later access tests its key's
//! bit in that record and its slot's hole. A call ends the run; so does a
//! write to the receiver's local.
use perry_codegen::{compile_module, CompileOptions};
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

/// x, y, z coloured at bits 0..2 and slots 0..2: every access of the fixture is a 2c access.
fn layouts() -> perry_codegen::ProgramLayouts {
    let mut l = perry_codegen::ProgramLayouts::default();
    for (i, k) in ["x", "y", "z"].iter().enumerate() {
        l.colored.insert((*k).into(), (i as u32, i as u32));
    }
    l.mask_words = 1;
    l
}

fn ir_of(source: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(source, "receiver_run.ts", &mut cache).unwrap();
    let module = lower_module(&parsed.module, "receiver_run", "receiver_run.ts").unwrap();
    let options = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        target: Some("x86_64-unknown-linux-gnu".into()),
        program_layouts: std::sync::Arc::new(layouts()),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, options).unwrap()).unwrap()
}

/// The body of the first function whose `define` line contains `name`.
fn body<'a>(ir: &'a str, name: &str) -> &'a str {
    let start = ir
        .find(&format!("{name}("))
        .and_then(|i| ir[..i].rfind("define "))
        .unwrap_or_else(|| panic!("no function {name}"));
    let end = ir[start..].find("\n}\n").map_or(ir.len(), |e| start + e);
    &ir[start..end]
}

fn labels(body: &str, prefix: &str) -> usize {
    body.lines().filter(|l| l.starts_with(prefix)).count()
}

const SRC: &str = r#"
class E {
  x: any; y: any; z: any;
  constructor() { this.x = 0; this.y = 0; this.z = 0; }
  set(x: any, y: any, z: any) { this.x = x; this.y = y; this.z = z; return this; }
  sum() { return this.x + this.y + this.z; }
  swap(v: any) { const a = this.x; this.y = v; this.z = a; return a; }
  split(x: any, f: () => void) { this.x = x; f(); this.y = x; this.z = x; }
}
function rsum(o: any) { const a = o.x; const b = o.y; const c = o.z; return a; }
function rswap(o: any, v: any) { const a = o.x; o.y = v; o.z = a; return a; }
function rnew(o: any, p: any) { const a = o.x; o = (0, p); const b = o.y; const c = o.z; return a; }
function rnewin(o: any, p: any) { const a = o.x; const b = (o = p).y; const c = o.z; return a; }
const e = new E();
console.log(rsum(e), rswap(e, 7), rnew(e, { x: 1, y: 2, z: 3 }), rnewin(e, { z: 1, y: 2 }));
e.set(1, 2, 3);
console.log(e.sum());
console.log(e.swap(5));
e.split(4, () => {});
"#;

#[test]
fn a_run_of_stores_proves_the_record_once() {
    let ir = ir_of(SRC);
    let set = body(&ir, "E__set");
    assert_eq!(
        labels(set, "l2s.word"),
        1,
        "one head for this.x/y/z:\n{set}"
    );
    assert_eq!(labels(set, "l2s.follow"), 2, "two followers:\n{set}");
}

#[test]
fn a_run_of_reads_proves_the_record_once() {
    let ir = ir_of(SRC);
    let sum = body(&ir, "rsum");
    assert_eq!(
        labels(sum, "l2c.word"),
        1,
        "one head for three reads:\n{sum}"
    );
    assert_eq!(labels(sum, "l2c.follow"), 2, "two followers:\n{sum}");
}

#[test]
fn a_call_ends_the_run() {
    let ir = ir_of(SRC);
    let split = body(&ir, "E__split");
    assert_eq!(
        labels(split, "l2s.word"),
        2,
        "the call starts a second head:\n{split}"
    );
    assert_eq!(
        labels(split, "l2s.follow"),
        1,
        "one follower after the call:\n{split}"
    );
}

/// A store tests the object's metadata word (a sealed intrinsic, a prototype
/// role or an exotic receiver takes the ordinary store); a read does not. The
/// first store of a run whose head is a read tests it once, and the stores
/// after it do not.
#[test]
fn the_first_store_of_a_read_run_tests_the_metadata_word_once() {
    let ir = ir_of(SRC);
    let swap = body(&ir, "rswap");
    assert_eq!(labels(swap, "l2c.word"), 1, "the read is the head:\n{swap}");
    assert_eq!(
        labels(swap, "l2s.follow"),
        2,
        "two store followers:\n{swap}"
    );
    assert_eq!(labels(swap, "l2s.meta"), 1, "one metadata test:\n{swap}");
    let set = body(&ir, "E__set");
    assert_eq!(
        labels(set, "l2s.meta"),
        1,
        "a store head proves it for the run:\n{set}"
    );
}

/// A write to the receiver's local ends its run: the access after it is a new
/// head (the new value may have any layout), never a follower of the record
/// proved for the old value.
/// `rnew` writes `o` with `(0, p)`, not `p`: a plain local-to-local copy
/// emits the string alias demote call, and that call alone ends the run, so
/// only a write that emits no call shows that the local write ends it.
#[test]
fn a_write_to_the_receiver_local_ends_the_run() {
    let ir = ir_of(SRC);
    let new = body(&ir, "rnew");
    assert_eq!(
        labels(new, "l2c.word"),
        2,
        "o.x heads the old value's run, o.y the new value's:\n{new}"
    );
    assert_eq!(labels(new, "l2c.follow"), 1, "only o.z follows:\n{new}");
    let within = body(&ir, "rnewin");
    assert_eq!(
        labels(within, "l2c.word"),
        3,
        "o.x, (o = p).y and o.z are three heads:\n{within}"
    );
    assert_eq!(
        labels(within, "l2c.follow"),
        0,
        "o.z does not follow o.x across the write:\n{within}"
    );
}
