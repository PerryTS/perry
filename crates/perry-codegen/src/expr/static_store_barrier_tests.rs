//! A store of a binary-image String literal (or a store-free selection of
//! literals, or a call of a function returning only such selections) takes
//! no write barrier; any other value keeps it (`with_static_child`).
//!
//! The stores are `this.f = v` in a class's methods, whose shape-proven body
//! lowers them inline with their barrier, so the counts below are about this
//! exemption alone.

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

/// The barrier calls in every body compiled for method `name` (the generic
/// body stores through the runtime; the shape-proven clone stores inline).
fn barriers(ir: &str, name: &str) -> usize {
    let symbol = format!("@perry_method_sbar__Box__{name}");
    let mut count = 0;
    let mut bodies = 0;
    let mut rest = ir;
    while let Some(at) = rest.find("\ndefine ") {
        let body = &rest[at + 1..];
        let end = body.find("\n}\n").map_or(body.len(), |e| e + 3);
        let head = body.lines().next().unwrap_or("");
        if head.contains(&format!("{symbol}(")) || head.contains(&format!("{symbol}$")) {
            bodies += 1;
            count += body[..end].matches("call void @js_write_barrier").count();
        }
        rest = &body[end..];
    }
    assert!(bodies > 0, "no body for {name}");
    count
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
