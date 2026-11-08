//! A store of a binary-image String literal (or a store-free selection of
//! literals, or a call of a function returning only such selections) takes
//! no write barrier; any other value keeps it (`with_static_child`).
//!
//! The stores are `this.f = v` in a class's methods: the shape-proven body
//! (`$pshape`) lowers them inline with their barrier, so the counts below
//! are about this exemption alone.

const SOURCE: &str = r#"
function pick(c: string): string { return c === "white" ? "black" : "white"; }
class Box {
  f: any = null;
  lit(): void { this.f = "white"; }
  sel(c: boolean): void { this.f = c ? "a" : "b"; }
  viaCall(c: string): void { this.f = pick(c); }
  obj(o: object): void { this.f = o; }
  storing(o: any): void { let t: any = 0; this.f = (t = o) ? "a" : t; }
}
const b = new Box();
b.lit(); b.sel(true); b.viaCall("white"); b.obj({}); b.storing(1);
console.log(b.f);
"#;

fn compile() -> String {
    let ast = perry_parser::parse_typescript(SOURCE, "sbar.ts").unwrap();
    let m = perry_hir::lower_module(&ast, "sbar", "sbar.ts").unwrap();
    String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

/// The barrier calls in method `name`'s shape-proven body.
fn barriers(ir: &str, name: &str) -> usize {
    let at = ir
        .find(&format!("@perry_method_sbar__Box__{name}$pshape("))
        .and_then(|p| {
            ir[..p]
                .rfind("define ")
                .filter(|d| !ir[*d..p].contains('\n'))
        })
        .unwrap_or_else(|| panic!("no body for {name}"));
    let end = ir[at..].find("\n}\n").unwrap() + at;
    ir[at..end].matches("call void @js_write_barrier").count()
}

#[test]
fn a_string_literal_store_takes_no_barrier() {
    let ir = compile();
    assert_eq!(barriers(&ir, "lit"), 0);
}

#[test]
fn a_selection_of_literals_or_a_literal_returning_call_takes_no_barrier() {
    let ir = compile();
    assert_eq!(barriers(&ir, "sel"), 0);
    assert_eq!(barriers(&ir, "viaCall"), 0);
}

#[test]
fn any_other_value_keeps_its_barrier() {
    let ir = compile();
    assert!(
        barriers(&ir, "obj") > 0,
        "an object store keeps its barrier"
    );
    // A store inside the selection is lowered here: no exemption.
    assert!(
        barriers(&ir, "storing") > 0,
        "a storing selection keeps its barrier"
    );
}
