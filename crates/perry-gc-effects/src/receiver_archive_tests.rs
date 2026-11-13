//! The receiver proof must survive real object relocations and mangled aliases.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[test]
fn archive_receiver_provenance_controls_the_erased_initializer() {
    use crate::{build_graph, classify, load_rules, Class};
    use std::process::Command;
    let key = "_RNvNtNtCsXbwMRDrjNy_13perry_runtime6object14canonical_keys14CANONICAL_KEYS";
    let good =
        "_RNvNvNtNtCsXbwMRDrjNy_13perry_runtime6object14canonical_keys14CANONICAL_KEYS7resolve";
    let badkey = "_RNvNtNtCsXbwMRDrjNy_13perry_runtime6object9prop_plan15READ_PLAN_CACHE";
    let bad = "_RNvNvNtNtCsXbwMRDrjNy_13perry_runtime6object9prop_plan15READ_PLAN_CACHE7resolve";
    let method = "_RNvMs5_NtCsXbwMRDrjNy_13perry_runtime7tls_hotINtB5_6HotKeyAAINtNtCsc1yEQwuZGGC_4core4cell4CellNtNtNtNtB7_6object13field_get_set7ic_miss17PrivateShapeProofEj2_j80_E17resolve_and_cacheB7_";
    let dir = tempfile::tempdir().unwrap();
    let asm = dir.path().join("receiver.s");
    let obj = dir.path().join("receiver.o");
    let archive = dir.path().join("receiver.a");
    let mut source = String::new();
    for (symbol, body) in [
        ("js_touch", format!("leaq {key}(%rip), %rdi\ncall {method}\nret")),
        ("js_widened", format!("testl %esi, %esi\nje 1f\nleaq {key}(%rip), %rdi\njmp 2f\n1: leaq {badkey}(%rip), %rdi\n2: call {method}\nret")),
        ("js_unknown", format!("call {method}\nret")),
        (method, "call *24(%rdi)\nret".into()),
        (good, "ret".into()),
        (bad, "call allocation\nret".into()),
        ("allocation", "ret".into()),
    ] {
        source.push_str(&format!(".section .text.{symbol},\"ax\",@progbits\n.globl {symbol}\n.type {symbol},@function\n{symbol}:\n{body}\n.size {symbol},.-{symbol}\n"));
    }
    for (symbol, target) in [(key, good), (badkey, bad)] {
        source.push_str(&format!(".section .data.{symbol},\"aw\",@progbits\n.globl {symbol}\n.type {symbol},@object\n{symbol}:\n.quad 0,0,0,{target}\n.size {symbol},.-{symbol}\n"));
    }
    std::fs::write(&asm, source).unwrap();
    assert!(Command::new("cc")
        .arg("-c")
        .arg(&asm)
        .arg("-o")
        .arg(&obj)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&obj)
        .status()
        .unwrap()
        .success());
    let rules = load_rules("js exact allocation -- fixture GC allocator\nindirect regex HotKey => regex ::resolve$ -- erased resolver union\n").unwrap();
    let graph = build_graph(&[archive]).unwrap();
    let classes = classify(&graph, &rules, None).unwrap();
    assert_eq!(classes.table["js_touch"].class, Class::Leaf);
    assert_eq!(classes.table["js_widened"].class, Class::Reenters);
    assert_eq!(classes.table["js_unknown"].class, Class::Reenters);
}
