// GUARDS1 (2026-11-26): the program-wide facts that let codegen drop the
// typed-view metadata word (`typed_metadata_written`) and the summary byte's
// prototype-modified bit (`array_methods_written`). A program that makes the
// write keeps the guard (`Some`); one that does not loses it (`None`).
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h: Vec<(String, Module)> = vec![(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

const PLAIN: &str = "const ta = new Float32Array(4); ta[0] = 1; const a = [1, 2]; a.push(ta.length + ta.byteLength); console.log(a.length, ta.byteOffset);";

#[test]
fn guards1_plain_program_writes_neither() {
    let pf = facts(PLAIN);
    assert_eq!(pf.typed_metadata_written, None);
    assert_eq!(pf.array_methods_written, None);
}

#[test]
fn guards1_typed_proto_length_getter() {
    let pf = facts(&format!("{PLAIN} Object.defineProperty(Object.getPrototypeOf(Float32Array.prototype), \"length\", {{ get() {{ return 1; }} }});"));
    assert!(pf.typed_metadata_written.is_some());
}

#[test]
fn guards1_typed_kind_proto_byte_length() {
    let pf = facts(&format!(
        "{PLAIN} Object.defineProperty(Float32Array.prototype, \"byteLength\", {{ value: 3 }});"
    ));
    assert!(pf.typed_metadata_written.is_some());
}

#[test]
fn guards1_typed_kind_proto_relinked() {
    let pf = facts(&format!(
        "{PLAIN} Object.setPrototypeOf(Float32Array.prototype, {{}});"
    ));
    assert!(pf.typed_metadata_written.is_some());
}

#[test]
fn guards1_typed_kind_proto_computed_store() {
    let pf = facts(&format!(
        "{PLAIN} (Float32Array.prototype as any)[process.argv[2] ?? \"q\"] = 1;"
    ));
    assert!(pf.typed_metadata_written.is_some());
}

#[test]
fn guards1_array_proto_named_add() {
    let pf = facts(&format!(
        "{PLAIN} (Array.prototype as any).sum = function () {{ return 0; }};"
    ));
    assert!(pf.array_methods_written.is_some());
}

#[test]
fn guards1_object_proto_store() {
    let pf = facts(&format!("{PLAIN} (Object.prototype as any).x = 1;"));
    assert!(pf.array_methods_written.is_some());
}

#[test]
fn guards1_other_builtin_write_keeps_both_facts() {
    // A write onto an unrelated builtin prototype withdraws neither fact.
    let pf = facts(&format!("{PLAIN} (Map.prototype as any).extra = 1;"));
    assert_eq!(pf.typed_metadata_written, None);
    assert_eq!(pf.array_methods_written, None);
}

#[test]
fn guards1_other_builtin_define_keeps_both_facts() {
    let pf = facts(&format!("{PLAIN} Object.defineProperty(Map.prototype, \"extra\", {{ value: 1 }}); delete (Set.prototype as any).add;"));
    assert_eq!(pf.typed_metadata_written, None);
    assert_eq!(pf.array_methods_written, None);
}

#[test]
fn guards1_store_reaching_object_proto_by_chain_withdraws() {
    // Object.getPrototypeOf(Map.prototype) is %Object.prototype%.
    let pf = facts(&format!(
        "{PLAIN} const op: any = Object.getPrototypeOf(Map.prototype); op.length = 1;"
    ));
    assert!(pf.typed_metadata_written.is_some());
    assert!(pf.array_methods_written.is_some());
}

#[test]
fn guards1_store_on_maybe_object_proto_withdraws() {
    let pf = facts(&format!("{PLAIN} const r: any = process.argv.length > 5 ? Map.prototype : Object.prototype; r.length = 1;"));
    assert!(pf.typed_metadata_written.is_some());
    assert!(pf.array_methods_written.is_some());
}

#[test]
fn guards1_array_proto_delete_withdraws() {
    let pf = facts(&format!("{PLAIN} delete (Array.prototype as any).push;"));
    assert!(pf.array_methods_written.is_some());
}

#[test]
fn guards1_typed_kind_proto_store_length() {
    let pf = facts(&format!(
        "{PLAIN} (Uint8Array.prototype as any).byteOffset = 5;"
    ));
    assert!(pf.typed_metadata_written.is_some());
}
