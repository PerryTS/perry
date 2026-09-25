//! #10714: a dynamically typed `.length` on a live plain Array is answered
//! from the receiver's GC header at the call site, instead of calling out to
//! the miss handler that could only answer it and cache nothing.
//!
//! Every assertion here would otherwise fail silently: without the arm the
//! program still computes the right length, through the slow entry, one call
//! per read.

use super::{emit_read, ir_opts};
use crate::compile_module;
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

/// `(label, body lines)` for every block of the first function that contains
/// `marker`, bodies trimmed and blank lines dropped. The chunk before the first
/// `define` is the preamble, whose `declare`s name every runtime helper, so a
/// marker must be something only a function BODY contains (a `call`, a label).
fn blocks_of(ir: &str, marker: &str) -> Vec<(String, Vec<String>)> {
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains(marker))
        .unwrap_or_else(|| panic!("no function contains `{marker}`:\n{ir}"));
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    for line in func.lines() {
        if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
            blocks.push((line.trim_end_matches(':').to_string(), Vec::new()));
        } else if let Some((_, body)) = blocks.last_mut() {
            if !line.trim().is_empty() {
                body.push(line.trim().to_string());
            }
        }
    }
    blocks
}

fn block<'a>(blocks: &'a [(String, Vec<String>)], prefix: &str) -> &'a (String, Vec<String>) {
    let found: Vec<_> = blocks
        .iter()
        .filter(|(label, _)| strip_suffix(label) == prefix)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one `{prefix}` block, found {}: {:?}",
        found.len(),
        blocks.iter().map(|(l, _)| l).collect::<Vec<_>>()
    );
    found[0]
}

/// Labels carry a numeric suffix (`pget.array_kind.12`); drop it.
fn strip_suffix(label: &str) -> String {
    let mut parts: Vec<&str> = label.split('.').collect();
    if parts.last().is_some_and(|p| p.parse::<u32>().is_ok()) {
        parts.pop();
    }
    parts.join(".")
}

/// `(cond, true_label, false_label)` of a block's conditional terminator.
fn cond_br(body: &[String]) -> (String, String, String) {
    let term = body
        .iter()
        .rev()
        .find(|l| l.starts_with("br "))
        .unwrap_or_else(|| panic!("block has no terminator: {body:?}"));
    let parts: Vec<&str> = term
        .strip_prefix("br i1 ")
        .unwrap_or_else(|| panic!("expected a conditional branch: {term}"))
        .split(", ")
        .collect();
    let label = |s: &str| s.trim_start_matches("label %").to_string();
    (parts[0].to_string(), label(parts[1]), label(parts[2]))
}

fn def<'a>(body: &'a [String], reg: &str) -> &'a str {
    let needle = format!("{reg} = ");
    body.iter()
        .find_map(|l| l.strip_prefix(needle.as_str()))
        .unwrap_or_else(|| panic!("`{reg}` is not defined in {body:?}"))
}

/// `cond` is `obj_type == GC_TYPE_ARRAY && !(gc_flags & GC_FLAG_FORWARDED)`,
/// both bytes loaded from the header in front of `handle`. Returns the register
/// holding the kind conjunct.
fn assert_header_test(body: &[String], cond: &str, handle: &str) -> String {
    let both = def(body, cond);
    assert!(both.starts_with("and i1 "), "{both}");
    let operands: Vec<&str> = both["and i1 ".len()..].split(", ").collect();
    let kind = def(body, operands[0]);
    let live = def(body, operands[1]);
    assert!(
        kind.starts_with("icmp eq i8 ") && kind.ends_with(", 1"),
        "the first conjunct is `obj_type == GC_TYPE_ARRAY`: {kind}"
    );
    assert!(
        live.starts_with("icmp eq i8 ") && live.ends_with(", 0"),
        "the second conjunct is the forwarded bit being clear: {live}"
    );
    let masked = live["icmp eq i8 ".len()..].split(", ").next().unwrap();
    assert!(
        def(body, masked).ends_with(", -128"),
        "the forwarded test must mask GC_FLAG_FORWARDED (0x80): {}",
        def(body, masked)
    );
    // Both bytes come from the header of `handle`, one byte below the other.
    for (reg, offset) in [
        (kind["icmp eq i8 ".len()..].split(", ").next().unwrap(), "8"),
        (
            def(body, masked)["and i8 ".len()..]
                .split(", ")
                .next()
                .unwrap(),
            "7",
        ),
    ] {
        let load = def(body, reg);
        assert!(load.starts_with("load i8, ptr "), "{load}");
        let ptr = load["load i8, ptr ".len()..].split(',').next().unwrap();
        let addr = def(
            body,
            def(body, ptr)["inttoptr i64 ".len()..]
                .split(' ')
                .next()
                .unwrap(),
        );
        assert_eq!(
            addr,
            format!("sub i64 {handle}, {offset}"),
            "header byte at handle-{offset}"
        );
    }
    operands[0].to_string()
}

/// The arm both paths share (`emit_plain_array_length_arm`), starting at the
/// block named `test_prefix`; returns the one label everything it refuses
/// continues to.
///
/// * the header test sends a live plain Array straight to `pget.array_length`;
/// * with `follow_stub` (the inline tower), a refused Array — so a forwarding
///   stub — follows ONE edge: the stub's first payload word, trusted only once
///   it is a heap address above the handle band, and re-checked as a live
///   plain Array. Anything else refused, at any of the three steps, leaves by
///   the same label. Without it (full-outline), the refusal leaves directly
///   and none of the stub blocks exist;
/// * `pget.array_length` loads the `u32` at the proved head (the phi of the two
///   handles, or the receiver itself) and converts it, with no call.
fn assert_plain_array_arm(
    blocks: &[(String, Vec<String>)],
    test_prefix: &str,
    follow_stub: bool,
) -> String {
    let (test_label, test_body) = block(blocks, test_prefix);
    let (cond, on_true, on_false) = cond_br(test_body);
    assert_eq!(strip_suffix(&on_true), "pget.array_length", "{test_body:?}");
    let kind_def = def(test_body, &cond);
    let handle = {
        let kind_reg = kind_def["and i1 ".len()..].split(", ").next().unwrap();
        let kind_load = def(test_body, kind_reg)["icmp eq i8 ".len()..]
            .split(", ")
            .next()
            .unwrap();
        let ptr = def(test_body, kind_load)["load i8, ptr ".len()..]
            .split(',')
            .next()
            .unwrap();
        let addr = def(
            test_body,
            def(test_body, ptr)["inttoptr i64 ".len()..]
                .split(' ')
                .next()
                .unwrap(),
        );
        addr["sub i64 ".len()..]
            .split(", ")
            .next()
            .unwrap()
            .to_string()
    };
    let kind = assert_header_test(test_body, &cond, &handle);

    if !follow_stub {
        for gone in [
            "pget.array_stub",
            "pget.array_stub_follow",
            "pget.array_stub_target",
        ] {
            assert!(
                !blocks.iter().any(|(l, _)| strip_suffix(l) == gone),
                "`{gone}` must not be emitted without the stub follow"
            );
        }
        let (_, load_body) = block(blocks, "pget.array_length");
        assert!(
            !load_body.iter().any(|l| l.contains(" = phi i64 ")),
            "{load_body:?}"
        );
        assert_length_load(load_body, &handle);
        return on_false;
    }
    assert_eq!(strip_suffix(&on_false), "pget.array_stub", "{test_body:?}");

    // Refused: only an Array (the kind conjunct) goes on to the stub follow.
    let (_, stub_body) = block(blocks, "pget.array_stub");
    let (stub_cond, stub_true, other) = cond_br(stub_body);
    assert_eq!(stub_cond, kind, "the stub test reuses the kind conjunct");
    assert_eq!(strip_suffix(&stub_true), "pget.array_stub_follow");

    // One edge: the forwarding word, admitted only as a heap address.
    let (_, follow_body) = block(blocks, "pget.array_stub_follow");
    let (follow_cond, follow_true, follow_false) = cond_br(follow_body);
    assert_eq!(strip_suffix(&follow_true), "pget.array_stub_target");
    assert_eq!(follow_false, other, "an untrusted forwarding word leaves");
    let target = follow_body
        .iter()
        .find(|l| l.contains(" = load i64, ptr "))
        .and_then(|l| l.split_once(" = "))
        .map(|(r, _)| r.to_string())
        .unwrap_or_else(|| panic!("the forwarding word is loaded: {follow_body:?}"));
    let word_ptr = def(follow_body, &target)["load i64, ptr ".len()..]
        .split(',')
        .next()
        .unwrap();
    assert_eq!(
        def(follow_body, word_ptr),
        format!("inttoptr i64 {handle} to ptr"),
        "the forwarding word is the stub's first payload word"
    );
    let candidate = def(follow_body, &follow_cond);
    let parts: Vec<&str> = candidate["and i1 ".len()..].split(", ").collect();
    let top_clear = def(follow_body, parts[0]);
    let top = top_clear["icmp eq i64 ".len()..]
        .split(", ")
        .next()
        .unwrap();
    assert!(top_clear.ends_with(", 0"), "{top_clear}");
    assert_eq!(def(follow_body, top), format!("lshr i64 {target}, 48"));
    assert_eq!(
        def(follow_body, parts[1]),
        format!("icmp ugt i64 {target}, 1048575"),
        "the destination must clear the handle band before its header is read"
    );

    // The destination is re-checked before anything of it is read.
    let (target_label, target_body) = block(blocks, "pget.array_stub_target");
    let (target_cond, target_true, target_false) = cond_br(target_body);
    assert_eq!(strip_suffix(&target_true), "pget.array_length");
    assert_eq!(
        target_false, other,
        "a longer chain leaves by the same edge"
    );
    assert_header_test(target_body, &target_cond, &target);

    // The load reads whichever head was proved live.
    let (_, load_body) = block(blocks, "pget.array_length");
    let phi = load_body
        .iter()
        .find(|l| l.contains(" = phi i64 "))
        .unwrap_or_else(|| panic!("the live head is a phi: {load_body:?}"));
    assert!(
        phi.contains(&format!("[ {handle}, %{test_label} ]"))
            && phi.contains(&format!("[ {target}, %{target_label} ]")),
        "{phi}"
    );
    let live = phi.split_once(" = ").unwrap().0;
    assert_length_load(load_body, live);
    other
}

/// `pget.array_length`: the `u32` at `live`'s payload offset 0, converted, and
/// an unconditional branch to the merge — no call.
fn assert_length_load(load_body: &[String], live: &str) {
    let len = load_body
        .iter()
        .find(|l| l.contains(" = load i32, ptr "))
        .unwrap_or_else(|| panic!("the length word is loaded: {load_body:?}"));
    let len_ptr = len.split(" = load i32, ptr ").nth(1).unwrap();
    assert_eq!(
        def(load_body, len_ptr.split(',').next().unwrap()),
        format!("inttoptr i64 {live} to ptr"),
        "`length` is the u32 at the live head's payload offset 0"
    );
    assert!(
        load_body.iter().any(|l| l.contains(" = uitofp i32 ")),
        "{load_body:?}"
    );
    assert!(
        !load_body.iter().any(|l| l.contains(" call ")),
        "the arm must not call out:\n{load_body:?}"
    );
    let term = load_body.last().unwrap();
    assert!(term.starts_with("br label %"), "{term}");
}

/// The inline tower: the header is read on the ShapeId compare's FALSE edge
/// and nowhere earlier, so a `.length` site's object hit path is untouched,
/// and a live plain Array is answered without reaching `pic.miss.call`.
#[test]
fn a_length_read_serves_a_live_plain_array_off_the_shape_compare() {
    let ir = emit_read("length");
    let blocks = blocks_of(&ir, "\npic.token");

    // Reached from `pic.token` only, on the FALSE edge of the compare whose
    // TRUE edge is the inline hit.
    let (kind_label, _) = block(&blocks, "pget.array_kind");
    let preds: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| {
            body.iter()
                .any(|l| l.starts_with("br ") && l.contains(&format!("label %{kind_label},")))
                || body
                    .iter()
                    .any(|l| l.starts_with("br ") && l.ends_with(&format!("label %{kind_label}")))
        })
        .map(|(label, _)| label.as_str())
        .collect();
    assert_eq!(
        preds.len(),
        1,
        "one edge may reach the array test: {preds:?}"
    );
    assert_eq!(strip_suffix(preds[0]), "pic.token", "{preds:?}");
    let (_, token_body) = block(&blocks, "pic.token");
    let (_, on_true, on_false) = cond_br(token_body);
    assert_eq!(strip_suffix(&on_true), "pic.hit", "{token_body:?}");
    assert_eq!(&on_false, kind_label, "{token_body:?}");

    // Everything the arm refuses — any other kind, or a forwarding stub it
    // cannot heal in one edge — continues exactly where the compare's false
    // edge used to go.
    let refused = assert_plain_array_arm(&blocks, "pget.array_kind", true);
    assert_eq!(strip_suffix(&refused), "pic.token.miss");

    // The merge takes the arm's value.
    let (load_label, load_body) = block(&blocks, "pget.array_length");
    let value = load_body
        .iter()
        .find(|l| l.contains(" = uitofp i32 "))
        .and_then(|l| l.split_once(" = "))
        .map(|(v, _)| v)
        .unwrap();
    let (_, merge_body) = block(&blocks, "pget.recv_merge");
    let phi = merge_body
        .iter()
        .find(|l| l.contains(" = phi double "))
        .unwrap();
    assert!(
        phi.contains(&format!("[ {value}, %{load_label} ]")),
        "the merge must take the array length from `{load_label}`:\n{phi}"
    );
    assert!(
        load_body.last().unwrap().contains("label %pget.recv_merge"),
        "{load_body:?}"
    );
}

/// The object HIT path of a `.length` site reads no GC header: an Array can
/// never match an object ShapeId (#10828, rule 3), so the kind test belongs
/// on the compare's false edge, and a site whose receivers are objects keeps
/// the exact instruction sequence every other key's hit has.
#[test]
fn a_length_site_hit_path_reads_no_gc_header() {
    let ir = emit_read("length");
    let blocks = blocks_of(&ir, "\npic.token");
    for prefix in ["pget.recv_ok", "pget.recv_obj", "pic.token", "pic.hit"] {
        let (_, body) = block(&blocks, prefix);
        assert!(
            !body.iter().any(|l| l.contains("load i8")),
            "`{prefix}` is on the object hit path and must not read the GC \
             header:\n{body:?}"
        );
    }
}

/// Only `.length` grows the arm: no other key has an answer in an Array's
/// header, so no other key may pay for the test.
#[test]
fn only_length_grows_the_array_arm() {
    for key in ["foo", "size", "charCodeAt"] {
        let ir = emit_read(key);
        assert!(
            !ir.contains("pget.array_kind") && !ir.contains("pget.array_length"),
            "`.{key}` must not grow the array-length arm:\n{ir}"
        );
    }
}

/// A module past the full-outline threshold, whose init reads `o.<property>`
/// on an `Any` local. The padding functions are what makes
/// `decide_full_outline_ic` choose the outlined helper; nothing references
/// them.
fn full_outline_module_reading(property: &str) -> Module {
    let mut m = Module::new("read_outlined.ts");
    m.functions = (0..4000u32)
        .map(|i| perry_hir::Function {
            id: 10_000 + i,
            name: format!("pad{i}"),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: perry_hir::types::Type::Void,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        })
        .collect();
    m.init = vec![
        Stmt::Let {
            id: 1,
            name: "o".to_string(),
            ty: perry_hir::types::Type::Any,
            mutable: false,
            init: Some(Expr::Undefined),
        },
        Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: property.to_string(),
            byte_offset: 0,
        }),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit_outlined_read(property: &str) -> String {
    let module = full_outline_module_reading(property);
    assert!(
        crate::codegen::decide_full_outline_ic(crate::codegen::module_callable_count(&module)),
        "test premise: the fixture must cross the full-outline threshold"
    );
    String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap())
        .expect("LLVM IR should be UTF-8")
}

/// Full-outline modules (#5391 path 3) answer a live plain Array's `.length`
/// ahead of `js_object_get_field_ic`; everything else still takes the call.
#[test]
fn a_full_outline_length_read_serves_a_live_plain_array_before_the_helper() {
    let ir = emit_outlined_read("length");
    let blocks = blocks_of(&ir, "call double @js_object_get_field_ic(");
    assert!(
        !blocks.iter().any(|(l, _)| l.starts_with("pic.")),
        "test premise: the read must be full-outlined, not the inline tower"
    );

    let refused = assert_plain_array_arm(&blocks, "pget.outline_array_header", false);
    assert_eq!(strip_suffix(&refused), "pget.outline_call");

    // The header is read only for a POINTER-tagged payload above the handle
    // band: the block that branches to the test checks exactly that.
    let (header_label, _) = block(&blocks, "pget.outline_array_header");
    let (_, entry_body) = blocks
        .iter()
        .find(|(_, body)| {
            body.last()
                .is_some_and(|t| t.contains(&format!("label %{header_label},")))
        })
        .expect("a block branches to the header test");
    let (cond, _, on_false) = cond_br(entry_body);
    assert_eq!(strip_suffix(&on_false), "pget.outline_call");
    let candidate = def(entry_body, &cond);
    let operands: Vec<&str> = candidate["and i1 ".len()..].split(", ").collect();
    assert!(
        def(entry_body, operands[0]).ends_with(", 32765"),
        "the exact POINTER_TAG test: {}",
        def(entry_body, operands[0])
    );
    assert!(
        def(entry_body, operands[1]).starts_with("icmp ugt i64 ")
            && def(entry_body, operands[1]).ends_with(", 1048575"),
        "the small-handle band test: {}",
        def(entry_body, operands[1])
    );

    // The helper is still the one call, and the merge joins the two answers.
    let (call_label, call_body) = block(&blocks, "pget.outline_call");
    let call = call_body
        .iter()
        .find(|l| l.contains("@js_object_get_field_ic("))
        .expect("the outlined helper is called from `pget.outline_call`");
    let call_value = call.split_once(" = ").map(|(v, _)| v).unwrap();
    let (load_label, load_body) = block(&blocks, "pget.array_length");
    let len_value = load_body
        .iter()
        .find(|l| l.contains(" = uitofp i32 "))
        .and_then(|l| l.split_once(" = "))
        .map(|(v, _)| v)
        .unwrap();
    let (_, merge_body) = block(&blocks, "pget.outline_merge");
    let phi = merge_body
        .iter()
        .find(|l| l.contains(" = phi double "))
        .expect("the outlined read merges its two answers");
    assert!(
        phi.contains(&format!("[ {len_value}, %{load_label} ]"))
            && phi.contains(&format!("[ {call_value}, %{call_label} ]")),
        "{phi}"
    );
}

/// Any other key in a full-outline module is the bare helper call it was.
#[test]
fn a_full_outline_non_length_read_is_the_bare_helper_call() {
    let ir = emit_outlined_read("foo");
    assert!(
        ir.contains("@js_object_get_field_ic("),
        "test premise: the read is full-outlined:\n{ir}"
    );
    for gone in [
        "pget.outline_array_header",
        "pget.array_length",
        "pget.outline_call",
        "pget.outline_merge",
    ] {
        assert!(!ir.contains(gone), "`.foo` must not grow `{gone}`");
    }
}
