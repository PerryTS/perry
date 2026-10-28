//! One collecting refusal per ordinary named read, with proofs left inline.
use super::{ir_opts, module_reading};
use crate::compile_module;
use perry_hir::{Expr, Module, Stmt};

type Blocks = Vec<(String, Vec<String>)>;

fn emit(properties: &[&str], colored: bool, target: &str) -> String {
    let mut module = module_reading(properties[0]);
    let receiver = match &module.init[1] {
        Stmt::Expr(Expr::PropertyGet { object, .. }) => object.clone(),
        other => panic!("expected generic read: {other:?}"),
    };
    module.init.truncate(1);
    for property in properties {
        module.init.push(Stmt::Expr(Expr::PropertyGet {
            byte_end: 0,
            object: receiver.clone(),
            property: (*property).into(),
            byte_offset: 0,
        }));
    }
    let mut opts = ir_opts(false, None);
    opts.target = Some(target.into());
    // No getter of the program is declared under these names: the reads
    // keep the stateless probe and own no shape/slot memo.
    opts.program_class_accessor_names =
        Some(std::sync::Arc::new(crate::ClassAccessorNames::default()));
    if colored {
        let mut layouts = crate::ProgramLayouts::default();
        layouts.colored.insert(properties[0].into(), (0, 0));
        layouts.mask_words = 1;
        opts.program_layouts = std::sync::Arc::new(layouts);
    }
    String::from_utf8(compile_module(&module, opts).unwrap()).unwrap()
}

fn blocks(ir: &str) -> Blocks {
    let function = ir
        .split("\ndefine ")
        .find(|f| f.contains("\npget.slow."))
        .unwrap();
    let mut out: Blocks = Vec::new();
    for line in function.lines() {
        if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
            out.push((line.trim_end_matches(':').into(), Vec::new()));
        } else if let Some((_, body)) = out.last_mut() {
            if !line.trim().is_empty() {
                body.push(line.trim().into());
            }
        }
    }
    out
}

fn block<'a>(blocks: &'a Blocks, prefix: &str) -> (&'a str, &'a [String]) {
    let found: Vec<_> = blocks
        .iter()
        .filter(|(label, _)| label.starts_with(prefix))
        .collect();
    assert_eq!(found.len(), 1, "{prefix}: {blocks:?}");
    (&found[0].0, &found[0].1)
}

fn calls<'a>(blocks: &'a Blocks, callee: &str) -> Vec<(&'a str, &'a str)> {
    let needle = format!("@{callee}(");
    let mut found = Vec::new();
    for (label, body) in blocks {
        for line in body {
            if line.contains(&needle) && (line.contains(" call ") || line.contains(" invoke ")) {
                found.push((label.as_str(), line.as_str()));
            }
        }
    }
    found
}

#[test]
fn shared_refusal_joins_the_receiver_failure_and_probe_decline() {
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        let ir = emit(&["value"], false, target);
        let blocks = blocks(&ir);
        let (slow_label, slow) = block(&blocks, "pget.slow.");
        let slow_calls = calls(&blocks, "js_object_get_field_value");
        assert_eq!(slow_calls.len(), 1, "{target}: {blocks:?}");
        assert_eq!(slow_calls[0].0, slow_label);
        assert!(calls(&blocks, "js_object_get_field_slow").is_empty());
        assert!(calls(&blocks, "js_object_get_field_nonptr").is_empty());
        let (other_label, other) = block(&blocks, "pget.recv_other.");
        assert_eq!(other, &[format!("br label %{slow_label}")]);
        let probe = calls(&blocks, "perry_probe_get");
        assert_eq!(probe.len(), 1, "{blocks:?}");
        assert!(
            probe[0].1.contains(" = call double "),
            "the probe remains a leaf call"
        );
        let probe_body = &blocks
            .iter()
            .find(|(label, _)| label == probe[0].0)
            .unwrap()
            .1;
        assert!(probe_body.iter().any(
            |line| line.contains("icmp ne i64") && line.ends_with(crate::nanbox::TAG_HOLE_I64)
        ));
        assert!(probe_body
            .last()
            .unwrap()
            .ends_with(&format!("label %{slow_label}")));
        let preds: Vec<_> = blocks
            .iter()
            .filter(|(_, body)| {
                body.last()
                    .is_some_and(|line| line.contains(&format!("label %{slow_label}")))
            })
            .map(|(label, _)| label.as_str())
            .collect();
        assert_eq!(preds.len(), 2, "{preds:?}");
        assert!(preds.contains(&other_label) && preds.contains(&probe[0].0));
        // The complete read receives the original unmasked value, never a
        // handle derived under the pointer proof that the other edge lacks.
        let bits = slow_calls[0]
            .1
            .split_once("(i64 ")
            .unwrap()
            .1
            .split(',')
            .next()
            .unwrap();
        assert!(blocks.iter().flat_map(|(_, b)| b).any(|line| line
            .starts_with(&format!("{bits} = bitcast double "))
            && line.ends_with(" to i64")));
        assert!(
            !slow.iter().any(|line| line.contains("add i64")),
            "no receiver handle on this join: {slow:?}"
        );
        let key = slow_calls[0]
            .1
            .split(", i64 ")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap();
        let key_def = slow
            .iter()
            .find(|line| line.starts_with(&format!("{key} = ")))
            .unwrap_or_else(|| panic!("key materializes on the refusal edge: {slow:?}"));
        if key_def.contains(" = and i64 ") {
            assert!(
                slow.iter().any(|line| line.contains("load double, ptr @")),
                "mutable key handle is reloaded on the refusal edge: {slow:?}"
            );
        } else {
            // Main's literal-image rewrite replaces the boxed handle's mask
            // with its immutable image address. That address stays cold too.
            let pointer = key_def
                .split_once(" = ptrtoint ptr ")
                .unwrap()
                .1
                .strip_suffix(" to i64")
                .unwrap();
            assert!(
                slow.iter().any(|line| line.starts_with(&format!(
                    "{pointer} = getelementptr i8, ptr @perry_image_literal_"
                ))),
                "literal key address materializes on the refusal edge: {slow:?}"
            );
        }
        let (_, merge) = block(&blocks, "pget.recv_merge.");
        let answer = slow_calls[0].1.split_once(" = ").unwrap().0;
        assert!(merge.iter().any(|line| line.contains("phi double")
            && line.contains(&format!("[ {answer}, %{slow_label} ]"))
            && !line.contains(&format!("%{other_label}"))));
        assert!(!ir.contains("@perry_ic_"), "no per-site state");
    }
}

#[test]
fn shared_refusal_is_per_site_and_keeps_the_colored_shape_proof_inline() {
    let ir = emit(&["value"], true, "x86_64-unknown-linux-gnu");
    let b = blocks(&ir);
    for prefix in ["l2c.word.", "l2c.record.", "l2c.load.", "l2c.hit."] {
        let (_, body) = block(&b, prefix);
        assert!(
            !body
                .iter()
                .any(|line| line.contains("@js_object_get_field")),
            "proof/hit must not collect: {body:?}"
        );
    }
    assert!(block(&b, "l2c.word.")
        .1
        .iter()
        .any(|line| line.contains("load i32")));
    assert!(block(&b, "l2c.record.")
        .1
        .iter()
        .any(|line| line.contains("and i64")));
    let slot = block(&b, "l2c.load.")
        .1
        .iter()
        .find(|line| line.contains(" = load i64, ptr "))
        .unwrap();
    let bits = slot.split_once(" = ").unwrap().0;
    assert!(
        block(&b, "l2c.hit.")
            .1
            .iter()
            .any(|line| line.contains(&format!(" = bitcast i64 {bits} to double"))),
        "the inline slot bits become the hit value: {b:?}"
    );
    assert_eq!(calls(&b, "perry_probe_get").len(), 1);
    assert_eq!(calls(&b, "js_object_get_field_value").len(), 1);
    let ir = emit(&["first", "second"], false, "x86_64-unknown-linux-gnu");
    let b = blocks(&ir);
    assert_eq!(calls(&b, "perry_probe_get").len(), 2);
    assert_eq!(calls(&b, "js_object_get_field_value").len(), 2);
}

#[test]
fn shared_refusal_preserves_length_and_required_global_entries() {
    let ir = emit(&["length"], false, "x86_64-unknown-linux-gnu");
    let b = blocks(&ir);
    assert!(calls(&b, "js_object_get_field_value").is_empty());
    assert_eq!(calls(&b, "js_object_get_field_slow").len(), 1);
    assert_eq!(calls(&b, "js_value_length_property_f64").len(), 1);
    assert_eq!(calls(&b, "perry_probe_get_length").len(), 1);
    for prefix in ["pget.recv_sso.", "pget.strlen_heap.", "pget.array_length."] {
        block(&b, prefix);
    }
    let mut module = Module::new("global_refusal.ts");
    module.init.push(Stmt::Expr(Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::GlobalGet(0)),
        property: "Object".into(),
        byte_offset: 0,
    }));
    let ir = String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap()).unwrap();
    let b = blocks(&ir);
    assert_eq!(calls(&b, "perry_probe_get_binding").len(), 1);
    assert_eq!(calls(&b, "js_global_get_field_slow").len(), 1);
    assert!(calls(&b, "js_object_get_field_value").is_empty());
}

#[test]
fn shared_refusal_keeps_collection_and_binding_probe_semantics() {
    for (key, probe) in [
        ("size", "perry_probe_get"),
        ("0", "perry_probe_get_binding"),
        ("#private", "perry_probe_get_binding"),
    ] {
        let ir = emit(&[key], false, "x86_64-unknown-linux-gnu");
        let b = blocks(&ir);
        assert_eq!(calls(&b, probe).len(), 1, "{key}: {b:?}");
        assert_eq!(calls(&b, "js_object_get_field_value").len(), 1);
        assert!(calls(&b, "js_object_get_field_nonptr").is_empty());
        if key == "size" {
            block(&b, "pget.collection_kind.");
            let (_, body) = block(&b, "pget.collection_size.");
            assert!(body.iter().any(|line| line.contains("load i32")));
            assert!(!body
                .iter()
                .any(|line| line.contains("@js_object_get_field")));
        }
    }
}
