//! Per-unit mode: a class whose parent is another unit's class holds the
//! parent's birth keys as its own layout prefix. The solve sees the class
//! with only its own keys until the parent settles, so a store of an
//! inherited birth key (the parent constructor's summary, a loop in a third
//! unit) is counted against the settled layout, not as a key add.

use super::*;

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

fn program(tail: &str) -> String {
    format!(
        "class Shape {{
  x: number; y: number;
  constructor(x: number, y: number) {{ this.x = x; this.y = y; }}
  area(): number {{ return 0; }}
  describe(): number {{ return this.area() + this.x + this.y; }}
}}
class Circle extends Shape {{
  r: number;
  constructor(x: number, y: number, r: number) {{ super(x, y); this.r = r; }}
  area(): number {{ return Math.PI * this.r * this.r; }}
  {tail}
}}
const shapes: Shape[] = [];
for (let i = 0; i < 10; i++) shapes.push(new Circle(i, i, 1));
let total = 0;
for (let i = 0; i < 10; i++) {{
  const s: Shape = shapes[i];
  s.x = (s.x + 1) % 10;
  total += s.describe();
}}
console.log(total);
"
    )
}

#[test]
fn inherited_birth_keys_are_no_key_adds_across_units() {
    let src = program("");
    let pf = units(&src);
    let whole = facts(&src);
    for t in ["Circle", "Shape"] {
        let (u, w) = (tmpl(&pf, t), tmpl(&whole, t));
        assert!(
            u.prefix_stable && u.fixed_layout && u.adds_keys == 0,
            "{t}\n{}",
            pf.dump_text()
        );
        assert_eq!(u.birth_keys, w.birth_keys, "{t}");
        assert_eq!(u.fixed_layout, w.fixed_layout, "{t}");
    }
}

#[test]
fn a_key_outside_the_settled_layout_is_still_an_add() {
    // `extra` is no birth key of Circle or of Shape: a method adds it after
    // construction. The layout is not fixed, per unit as whole.
    let src = format!(
        "{}
(shapes[0] as Circle).grow();
",
        program("grow(): void { (this as any).extra = 1; }")
    );
    let pf = units(&src);
    let whole = facts(&src);
    assert!(!tmpl(&whole, "Circle").fixed_layout, "{}", whole.dump_text());
    let c = tmpl(&pf, "Circle");
    assert!(!c.fixed_layout && c.adds_keys > 0, "{}", pf.dump_text());
}

#[test]
fn a_delete_of_an_inherited_key_keeps_the_layout_open() {
    let src = format!(
        "{}\ndelete (shapes[0] as any).x;\n",
        program("")
    );
    let pf = units(&src);
    let c = tmpl(&pf, "Circle");
    assert!(!c.fixed_layout, "{}", pf.dump_text());
}
