#!/usr/bin/env python3
"""Break each round-two proof, require its focused witness to fail, restore."""
import argparse
import os
from pathlib import Path
import subprocess

p = argparse.ArgumentParser()
p.add_argument("--hostdir", type=Path, required=True)
root = p.parse_args().hostdir.resolve()
src = root / "src"
faults = [
    ("object/method_site.rs",
     "prime(slot, recv_h.get_nanbox_f64(), name, argc); // pre-call shape",
     "let _ = (slot, recv_h.get_nanbox_f64(), name, argc);",
     "a_mutating_body_primes_the_shape_seen_before_the_call"),
    ("object/method_site/read_holder/shared.rs", "if !self.exact_proof().valid() {", "if false {",
     "deep_method_entries_prime_and_guard_shadowing_and_trace_the_chain"),
    ("proxy/put_value/packed_add.rs", "&& (*proof).links.valid()", "&& true",
     "unrelated_generation_keeps_the_memo_but_a_changed_holder_refuses_it"),
    ("object/method_site/read_holder/function_own.rs",
     "return (bits != crate::value::TAG_UNDEFINED && bits != crate::value::TAG_HOLE)",
     "return (bits != crate::value::TAG_HOLE)",
     "native_alias_own_slots_are_receiver_relative_and_decline_undefined"),
    ("object/method_site/read_holder/class_read.rs",
     "        forward_absent,\n        exact_chain: false,",
     "        forward_absent: false,\n        exact_chain: false,",
     "native_alias_absence_primes_but_keeps_collecting_forwarding"),
    ("object/class_registry/class_meta.rs",
     "let declared = declaration_has_identity(class_id);\n        *role",
     "let declared = false;\n        *role",
     "declaration_role_is_projected_for_every_registration_order"),
    ("object/shape_chain.rs",
     "visitor.visit_tagged_usize_slot(addr, crate::value::POINTER_TAG);",
     "let _ = addr;",
     "chain_proofs_guard_each_hop_and_trace_every_holder"),
    ("proxy/put_value/packed_add.rs",
     "|| crate::object::field_rep_store::constfn_store_info(value.to_bits()) != Some(body)",
     "|| false",
     "packed_add_serves_only_the_shape_body_of_a_special_successor"),
    ("perry-codegen:expr/object_literal.rs",
     "if props.is_empty() {",
     "if false {",
     "empty_literal_builders_start_with_an_ordinary_shape"),
    ("object/read_stub.rs",
     "Some(crate::object::shapes::ShapeObjectKind::OrdinaryNativeAlias)",
     "Some(crate::object::shapes::ShapeObjectKind::Ordinary)",
     "global_stub_refuses_alias_publication_and_a_pre_alias_entry"),
    ("proxy/put_value/packed_add.rs",
     "if typed_flags != 0 && add_rep_conflicts(site, pre, typed_flags, constfn_body.unwrap_or(0)) {",
     "if false {",
     "different_bodies_at_one_pre_shape_share_one_transition"),
    ("object/alloc_basic.rs", "    object_alloc_plain(field_count)\n}",
     "    js_object_alloc(0, field_count)\n}",
     "empty_literal_birth_is_plain_before_its_first_shape"),
    ("proxy/put_value/packed_add.rs",
     "if site.is_null() || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT {",
     "if site.is_null() {",
     "a_worker_with_seeded_ids_cannot_consume_a_primary_add_proof"),
    ("perry-codegen:expr/put_value_store_ic.rs",
     "ctx.block().cond_br(&solo, &agent_label, miss_label);",
     "ctx.block().cond_br(\"true\", &agent_label, miss_label);",
     "key_add_shape_proofs_are_loads_and_keep_the_generic_miss"),
    ("perry-codegen:expr/shape_chain.rs",
     "ctx.block().icmp_eq(I64, &actual, &expected)",
     '"true".to_string()',
     "short_chain_prefixes_compare_each_holder_word"),
    ("proxy/put_value/packed_add.rs", "if rep_word != 0 && typed_flags == 0 {", "if false {",
     "an_any_append_still_guards_a_typed_prefix"),
    ("object/method_site/read_holder/class_read.rs",
     "super::function_own::alias_own_answer(e, recv, token)\n            .or_else",
     "super::saved_entry_answer(e, token, recv)\n            .or_else",
     "saved_alias_undefined_slots_keep_their_collecting_answer"),
    ("object/class_registry/class_meta.rs",
     "crate::object::shapes::class_identity_proto_id(class_id),\n        ) != 0",
     "crate::object::shapes::class_identity_proto_id(class_id),\n        ) == 0",
     "declaration_role_survives_member_retirement"),
    ("object/shape_chain.rs",
     ".all(|hop| std::ptr::read(hop.addr as *const u64) == hop.word)",
     ".all(|hop| !hop.heap || std::ptr::read(hop.addr as *const u64) == hop.word)",
     "class_shape_chain_guards_the_live_identity_word"),
    ("object/shape_chain.rs",
     "visitor.visit_nanbox_u64_slot(&mut hop.word);",
     "let _ = &mut hop.word;",
     "class_shape_chain_guards_the_live_identity_word"),
    ("object/method_site.rs", "if info == METHOD_SITE_VALUE_INFO {\n        actual != 0\n    } else {",
     "if info == METHOD_SITE_VALUE_INFO {\n        false\n    } else {",
     "value_method_entry_serves_changed_bodies_without_eviction_and_declines_noncallables"),
    ("perry-codegen:expr/method_site.rs",
     'let has_info = ctx.block().icmp_ne(I64, &info, "0");',
     'let has_info = "true".to_string();',
     "value_method_entries_check_closure_kind_and_info_without_a_body_guard"),
    ("object/method_site/holder_prime.rs",
     "if old.info != new.info && new.slot & METHOD_SITE_CONSTFN == 0 {",
     "if false {",
     "direct_method_body_changes_widen_the_existing_slot_instead_of_consuming_ways"),
    ("object/method_site/holder_prime.rs",
     "if *captures.add(count - 1) != this.as_f64().to_bits() {",
     "if true {",
     "value_method_invocation_reuses_matching_this_and_rebinds_a_different_receiver"),
    ("perry-codegen:expr/shape_chain.rs",
     'let four = ctx.block().icmp_eq(I64, &len, "4");',
     'let four = "true".to_string();',
     "four_hop_store_proofs_require_exact_length_and_compare_every_word"),
    ("perry-codegen:expr/shape_chain.rs",
     'let i = ctx.block().phi(I64, &[("4", &deep_l), (&next, &check_l)]);',
     'let i = ctx.block().phi(I64, &[("5", &deep_l), (&next, &check_l)]);',
     "four_hop_store_proofs_require_exact_length_and_compare_every_word"),
]
# Prove the unmodified witness is green before accepting its fault verdict.
# A stale positive assertion must never stand in for detecting a mutation.
positives = dict.fromkeys(
    ((relative.split(":", 1)[0] if ":" in relative else "perry-runtime"), witness)
    for relative, _, _, witness in faults
)
for crate, witness in positives:
    log = root / f"positive-control-{witness}.log"
    with log.open("w") as stream:
        result = subprocess.run(
            ["taskset", "-c", "0-55", "cargo", "test", "--release", "-j8",
             "-p", crate, "--lib", witness, "--", "--test-threads=1"], cwd=src,
            env=dict(os.environ, RUST_TEST_THREADS="1"), stdout=stream,
            stderr=subprocess.STDOUT, timeout=1800)
    evidence = log.read_text()
    if result.returncode or "running 1 test" not in evidence or "1 passed; 0 failed" not in evidence:
        raise RuntimeError(f"candidate witness is not green: {witness}; inspect {log}")
    print(f"POSITIVE: {witness}", flush=True)

# A wrong-body cached store itself generalizes the shape. An unguarded leaf
# undefined answer can also mask the saved collecting-slot witness. Keep
# those faults separate from the witnesses they could mask.
body_fault = faults[7]
saved_alias_fault = faults[16]
visibility_fault = faults[17]
deep_guard_fault = faults[1]
plain_birth_fault = faults[11]
identity_guard_fault = faults[18]
identity_root_fault = faults[19]
tail_induction_fault = faults[25]
for group_index, group in enumerate([[fault for fault in faults if fault not in (body_fault, saved_alias_fault, visibility_fault, deep_guard_fault, plain_birth_fault, identity_guard_fault, identity_root_fault, tail_induction_fault)],
              [body_fault], [saved_alias_fault, deep_guard_fault, plain_birth_fault, identity_guard_fault], [visibility_fault], [identity_root_fault], [tail_induction_fault]]):
    originals = {}
    try:
        if faults[15] in group:
            # This witness targets the successor representation, independently
            # of the separately sabotaged exported plain-birth wrapper. Its
            # candidate wrapper is exactly this existing birth primitive.
            path = src / "crates/perry-runtime/src/proxy/put_value/packed_add_tests.rs"
            original = path.read_text()
            before = ('let prefix = interned(b"mixed_prefix_number");\n'
                      '    let key = interned(b"mixed_any_append");\n'
                      '    let first = crate::object::js_object_alloc_plain(0);')
            if original.count(before) != 1:
                raise RuntimeError("typed-prefix witness setup changed")
            originals[path] = original
            path.write_text(original.replace(before,
                before.replace("::js_object_alloc_plain(", "::object_alloc_plain("), 1))
        for relative, before, after, _ in group:
            crate, relative = relative.split(":", 1) if ":" in relative else ("perry-runtime", relative)
            path = src / "crates" / crate / "src" / relative
            original = path.read_text()
            if original.count(before) != 1:
                raise RuntimeError(f"sabotage anchor changed: {relative}")
            originals.setdefault(path, original)
            path.write_text(original.replace(before, after, 1))
        for relative, _, _, witness in group:
            crate = relative.split(":", 1)[0] if ":" in relative else "perry-runtime"
            log = root / f"negative-control-g{group_index}-{witness}.log"
            with log.open("w") as stream:
                result = subprocess.run(
                    ["taskset", "-c", "0-55", "cargo", "test", "--release", "-j8",
                     "-p", crate, "--lib", witness], cwd=src,
                    env=dict(os.environ, RUST_TEST_THREADS="1"), stdout=stream,
                    stderr=subprocess.STDOUT, timeout=1800)
            evidence = log.read_text()
            if result.returncode == 0 or "running 1 test" not in evidence or "FAILED" not in evidence:
                raise RuntimeError(f"witness did not detect sabotage: {witness}; inspect {log}")
            print(f"DETECTED: {witness}", flush=True)
    finally:
        for path, original in originals.items():
            path.write_text(original)
        print("Restored candidate sources", flush=True)
