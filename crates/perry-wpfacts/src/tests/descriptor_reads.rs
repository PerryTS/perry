//! ToPropertyDescriptor reads `enumerable`, `configurable`, `value`,
//! `writable`, `get` and `set` of the descriptor with [[Get]]
//! (`DESCRIPTOR_KEYS`): a getter under any of them runs, not only under the
//! keys the definition uses (SOUND10 D, test_gap_gc_define_property_
//! descriptor_rooting arm 3: the `enumerable` and `configurable` getters
//! were claimed inactive while Node ran them).
use super::*;

/// Which of `names` run.
fn running(src: &str, names: &[&'static str]) -> Vec<&'static str> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    names
        .iter()
        .copied()
        .filter(|n| {
            let id = hir.functions.iter().find(|f| f.name == *n).expect("fn").id;
            pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
        })
        .collect()
}

const FLAGS: [&str; 3] = ["onEnumerable", "onConfigurable", "onWritable"];

/// A descriptor whose `enumerable`, `configurable` and `writable` are
/// getters, given to `DEFINE`.
fn program(define: &str) -> String {
    format!(
        r#"
function onEnumerable() {{ return true; }}
function onConfigurable() {{ return true; }}
function onWritable() {{ return true; }}
const target: any = {{}};
const k: string = "key-" + target.length;
const descriptor: any = {{ value: 1 }};
Object.defineProperty(descriptor, "enumerable", {{ get: function () {{ return onEnumerable(); }} }});
Object.defineProperty(descriptor, "configurable", {{ get: function () {{ return onConfigurable(); }} }});
Object.defineProperty(descriptor, "writable", {{ get: function () {{ return onWritable(); }} }});
{define}
console.log(Object.keys(target).length);
"#
    )
}

#[test]
fn a_definition_reads_every_descriptor_key() {
    for define in [
        "Object.defineProperty(target, k, descriptor);",
        "Object.defineProperty(target, \"x\", descriptor);",
        "Object.defineProperties(target, { [k]: descriptor });",
        "Object.defineProperties(target, { x: descriptor });",
        "Reflect.defineProperty(target, k, descriptor);",
    ] {
        assert_eq!(running(&program(define), &FLAGS), FLAGS, "{define}");
    }
}

/// Control: a descriptor no definition reads runs no getter.
#[test]
fn an_unread_descriptor_runs_no_getter() {
    assert!(running(&program("target[k] = typeof descriptor;"), &FLAGS).is_empty());
}
