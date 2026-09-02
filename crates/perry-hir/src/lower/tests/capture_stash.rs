//! Derived-ctor capture-stash placement (#8630): the `this.__perry_cap_*`
//! stash must follow `super()`, not constructor entry. Split from `tests.rs`
//! for the 2000-line file cap.

/// A derived class with captured outers whose `super()` is not its own
/// statement — the minifier's `super(a), this.x = b, …` comma sequence, as in
/// Next's `AppRouteRouteModule` — must stash the `this.__perry_cap_*` fields
/// AFTER the call, not at constructor entry. #8630's derived-`this` TDZ turns
/// an entry stash into `ReferenceError: Must call super constructor …` at
/// every construction (the Coop Next.js fixture died at module init).
#[test]
fn derived_ctor_capture_stash_follows_super_inside_comma_sequence() {
    let source = r#"
        const exported = (() => {
            const shared = { tag: "outer" };
            class Base {
                constructor(opts) { this.definition = opts.definition; }
            }
            class Derived extends Base {
                constructor({ definition: r, name: n }) {
                    super({ definition: r }), this.name = n, this.tag = shared.tag;
                }
            }
            return Derived;
        })();
    "#;
    assert_capture_stash_follows_super(source, "Derived");
}

/// Same requirement for a `super()` nested deeper than a leading comma operand
/// — p-queue's `if (super(), this.a = 0, …)` shape.
#[test]
fn derived_ctor_capture_stash_follows_super_inside_if_test() {
    let source = r#"
        const exported = (() => {
            const shared = { tag: "outer" };
            class Base {
                constructor() { this.base = 1; }
            }
            class Derived extends Base {
                constructor(e) {
                    var q;
                    if (super(), this.count = 0, this.tag = shared.tag, !e) { q = 1; }
                    this.q = q;
                }
            }
            return Derived;
        })();
    "#;
    assert_capture_stash_follows_super(source, "Derived");
}

fn assert_capture_stash_follows_super(source: &str, class_name: &str) {
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let class = hir
        .classes
        .iter()
        .find(|c| c.name == class_name)
        .unwrap_or_else(|| panic!("fixture declares class {class_name}"));
    let ctor = class
        .constructor
        .as_ref()
        .expect("the derived class keeps its user-written constructor");
    let mut super_at = None;
    let mut first_stash_at = None;
    for (index, stmt) in ctor.body.iter().enumerate() {
        let compact: String = format!("{stmt:?}")
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        if super_at.is_none() && compact.contains("SuperCall(") {
            super_at = Some(index);
        }
        if first_stash_at.is_none()
            && compact.contains("PropertySet{object:This,property:\"__perry_cap_")
        {
            first_stash_at = Some(index);
        }
    }
    // Anti-vacuity: the fixture must actually capture (`shared`) and call
    // `super()`, or the ordering below is not being tested.
    let super_at = super_at.expect("fixture constructor calls super()");
    let first_stash_at = first_stash_at.expect("fixture class captures an outer local");
    assert!(
        first_stash_at > super_at,
        "capture stash (stmt {first_stash_at}) must follow super() (stmt {super_at}): {:#?}",
        ctor.body
    );
}
