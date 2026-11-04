//! A class evaluation's self-binding owner (`ClassExprFresh::evaluation_owner`)
//! is initialized through the binding's real storage.
//!
//! `C = class Inner extends base() { static make = () => new Inner() }` in a
//! function: the static field arrow captures `Inner` and nothing writes it, so
//! the owner local is BOXED (its frame slot holds the box). The evaluation
//! used to store the class value straight into that frame slot, replacing the
//! box pointer; the HIR's `LocalSet(owner, <class>)` then wrote the class
//! through the "box" into the class object itself and `C` read back a word of
//! the class object (effect's `SocketUpgradeError`: no statics, no
//! inherited `ast`, "Cannot read properties of undefined (reading 'encoding')").
use crate::testing::root_slots::function_slice;
use crate::{compile_module, user_function_symbol};

fn lower_src(src: &str) -> perry_hir::Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "owner.ts", &mut cache)
                .expect("parse should succeed");
            perry_hir::set_current_module_source(src);
            let module = perry_hir::lower_module(&parsed.module, "owner", "owner.ts")
                .expect("lowering should succeed");
            perry_hir::clear_current_module_source();
            module
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

fn function_ir(src: &str, name: &str) -> String {
    let module = lower_src(src);
    let symbol = user_function_symbol(&module.name, name);
    let ir = String::from_utf8(
        compile_module(&module, crate::temp_root_coverage::entry_opts())
            .expect("owner fixture compiles"),
    )
    .unwrap();
    function_slice(&ir, &symbol).to_string()
}

/// The frame slot holding the box allocated by `js_box_alloc_bits`: the
/// first slot the box handle is stored into.
fn box_slot(fn_ir: &str) -> Option<String> {
    let mut lines = fn_ir
        .lines()
        .skip_while(|l| !l.contains("@js_box_alloc_bits("));
    lines.next()?;
    lines.find_map(|l| {
        let t = l.trim();
        let rest = t.strip_prefix("store ")?;
        if rest.contains(" null,") {
            return None;
        }
        Some(rest.rsplit_once(", ptr ")?.1.trim().to_string())
    })
}

const SOURCE: &str = r#"
function base() { return class B { static get ast() { return 1; } }; }
export function make() {
  const C = class Inner extends base() {
    static unsupported = () => new Inner();
  };
  return C;
}
make();
"#;

#[test]
fn a_boxed_self_binding_owner_is_initialized_through_its_box() {
    let ir = function_ir(SOURCE, "make");
    let slot = box_slot(&ir).unwrap_or_else(|| {
        panic!(
            "the captured self binding is boxed:
{ir}"
        )
    });
    // The slot holds the box handle for the binding's whole life: apart from
    // the handle itself (and frame-exit clears), nothing may be stored into it.
    let target = format!(", ptr {slot}");
    let stores: Vec<&str> = ir
        .lines()
        .map(str::trim)
        .filter(|t| t.starts_with("store ") && t.ends_with(&target) && !t.contains(" null,"))
        .collect();
    assert_eq!(
        stores.len(),
        1,
        "only the box handle may be stored into the owner's slot {slot}: {stores:#?}
{ir}"
    );
    // The evaluation initializes the self binding through the box BEFORE the
    // static initializers run (the arrow `unsupported` is created after it).
    let evaluated = ir.find("@js_class_evaluation_object").unwrap_or_else(|| {
        panic!(
            "fresh class evaluation:
{ir}"
        )
    });
    let arrow = ir[evaluated..]
        .find("@perry_closure_")
        .map(|i| i + evaluated)
        .unwrap_or_else(|| {
            panic!(
                "static field arrow:
{ir}"
            )
        });
    assert!(
        ir[evaluated..arrow].contains("@js_box_set_bits("),
        "the self binding must be written through its box before static initializers:
{ir}"
    );
}
