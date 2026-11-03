//! #10714: a dynamically typed `.length` on a live plain Array is answered
//! from the receiver's GC header at the call site, instead of calling out to
//! the miss handler that could only answer it and cache nothing.
//!
//! Every assertion here would otherwise fail silently: without the arm the
//! program still computes the right length, through the slow entry, one call
//! per read.

use super::emit_read;

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
        format!("icmp ugt i64 {target}, 2097151"),
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

/// The header is read on the object path only, after the receiver is proved a
/// real heap object (`pget.recv_obj` -> `pget.recv_real`), and a live plain
/// Array is answered without reaching the probe or the slow call. Everything
/// the arm refuses continues to the length probe (`pget.array_other`), whose
/// own decline is the one collecting exit.
#[test]
fn a_length_read_serves_a_live_plain_array_off_the_shape_compare() {
    let ir = emit_read("length");
    let blocks = blocks_of(&ir, "\npget.slow");

    // Reached from `pget.recv_obj` only, on the TRUE edge of the handle-band
    // test (the FALSE edge is the slow exit).
    let (real_label, _) = block(&blocks, "pget.recv_real");
    let preds: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| {
            body.iter()
                .any(|l| l.starts_with("br ") && l.contains(&format!("label %{real_label}")))
        })
        .map(|(label, _)| label.as_str())
        .collect();
    assert_eq!(
        preds.len(),
        1,
        "one edge may reach the array test: {preds:?}"
    );
    assert_eq!(strip_suffix(preds[0]), "pget.recv_obj", "{preds:?}");
    let (_, obj_body) = block(&blocks, "pget.recv_obj");
    let (_, on_true, on_false) = cond_br(obj_body);
    assert_eq!(&on_true, real_label, "{obj_body:?}");
    assert_eq!(strip_suffix(&on_false), "pget.slow", "{obj_body:?}");

    // Everything the arm refuses — any other kind, or a forwarding stub it
    // cannot heal in one edge — continues to the length probe.
    let refused = assert_plain_array_arm(&blocks, "pget.recv_real", true);
    assert_eq!(strip_suffix(&refused), "pget.array_other");

    // The probe is the `length` flavour, reads this agent's directory, and
    // only its TAG_HOLE decline reaches the collecting exit.
    let (other_label, other_body) = block(&blocks, "pget.array_other");
    let probe = other_body
        .iter()
        .find(|l| l.contains(" = call double @perry_probe_get_length("))
        .unwrap_or_else(|| panic!("expected the length probe call:\n{other_body:?}"));
    assert!(
        probe.contains("@perry_probe_get_length(ptr %"),
        "the length probe must receive the agent's own directory:\n{probe}"
    );
    assert!(
        other_body
            .iter()
            .any(|l| l.contains("getelementptr i8, ptr @PERRY_AGENT_PTRS, i64 0")),
        "the directory is slot 0 of the agent block:\n{other_body:?}"
    );
    let (cond, served, declined) = cond_br(other_body);
    assert!(
        def(other_body, &cond).starts_with("icmp ne i64 ")
            && def(other_body, &cond).ends_with(crate::nanbox::TAG_HOLE_I64),
        "{other_body:?}"
    );
    assert_eq!(strip_suffix(&served), "pget.recv_merge");
    assert_eq!(strip_suffix(&declined), "pget.slow");

    // The merge takes the arm's value and the probe's answer.
    let (load_label, load_body) = block(&blocks, "pget.array_length");
    let value = load_body
        .iter()
        .find(|l| l.contains(" = uitofp i32 "))
        .and_then(|l| l.split_once(" = "))
        .map(|(v, _)| v)
        .unwrap();
    let answer = probe.split_once(" = ").unwrap().0;
    let (_, merge_body) = block(&blocks, "pget.recv_merge");
    let phi = merge_body
        .iter()
        .find(|l| l.contains(" = phi double "))
        .unwrap();
    assert!(
        phi.contains(&format!("[ {value}, %{load_label} ]"))
            && phi.contains(&format!("[ {answer}, %{other_label} ]")),
        "the merge must take the array length and the probe's answer:\n{phi}"
    );
    assert!(
        load_body.last().unwrap().contains("label %pget.recv_merge"),
        "{load_body:?}"
    );
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
