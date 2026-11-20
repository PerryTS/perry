//! Buffer alias-scope metadata numbering. Every numbered node takes its id
//! from the module's one counter; the old fixed bases (scopes at 101+i,
//! scope lists at 201+i, noalias lists at 301+i) defined `!201` twice once a
//! module held more than 100 scopes.

use std::collections::HashMap;

use super::emit_buffer_alias_metadata;
use crate::module::LlModule;

/// Lower `per_function` scope counts the way function lowering does
/// (allocate from the module counter, commit after the body), then emit the
/// alias tables and the loop IDs. Returns every scope id.
fn module_with_scopes(per_function: &[usize]) -> (LlModule, Vec<u32>) {
    let mut llmod = LlModule::new(crate::codegen::default_target_triple());
    let mut all = Vec::new();
    for &count in per_function {
        let mut scopes = llmod.buffer_alias_scope_allocator();
        for _ in 0..count {
            all.push(scopes.allocate());
        }
        llmod.commit_buffer_alias_scopes(scopes);
    }
    emit_buffer_alias_metadata(&mut llmod);
    llmod.number_loop_metadata();
    (llmod, all)
}

fn numbered_definitions(ir: &str) -> HashMap<u32, usize> {
    let mut defs = HashMap::new();
    for line in ir.lines() {
        let Some(rest) = line.strip_prefix('!') else {
            continue;
        };
        let Some((id, _)) = rest.split_once(" = ") else {
            continue;
        };
        if let Ok(id) = id.parse::<u32>() {
            *defs.entry(id).or_insert(0) += 1;
        }
    }
    defs
}

#[test]
fn more_than_a_hundred_buffer_scopes_define_every_id_once() {
    // 3 functions, 150 scopes: past both old 100-id bands.
    let (llmod, scopes) = module_with_scopes(&[60, 0, 90]);
    assert_eq!(scopes.len(), 150);
    let ir = llmod.to_ir();
    let defs = numbered_definitions(&ir);
    let twice: Vec<_> = defs
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(id, _)| *id)
        .collect();
    assert!(
        twice.is_empty(),
        "metadata ids defined more than once: {twice:?}"
    );
    for &scope in &scopes {
        for id in [scope, scope + 1, scope + 2] {
            assert_eq!(
                defs.get(&id),
                Some(&1),
                "!{id} of scope !{scope} is not defined once"
            );
        }
        // The node is a scope, its list names it, its noalias list omits it.
        let line = |id: u32| {
            ir.lines()
                .find(|l| l.starts_with(&format!("!{id} = ")))
                .unwrap()
                .to_string()
        };
        assert!(line(scope).contains("distinct"), "{}", line(scope));
        assert_eq!(line(scope + 1), format!("!{} = !{{!{scope}}}", scope + 1));
        assert!(
            !line(scope + 2).contains(&format!("!{scope},"))
                && !line(scope + 2).ends_with(&format!("!{scope}}}"))
        );
    }
}

#[cfg(feature = "llvm-inprocess")]
#[test]
fn llvm_parses_a_module_with_more_than_a_hundred_buffer_scopes() {
    let (llmod, scopes) = module_with_scopes(&[130]);
    let mut ir = llmod.to_ir();
    // One access per scope, tagged exactly as the GEP fast path tags it.
    ir.push_str("define void @touch(ptr %p) {\nentry:\n");
    for (i, &scope) in scopes.iter().enumerate() {
        ir.push_str(&format!(
            "  %v{i} = load i8, ptr %p, align 1{}\n",
            crate::expr::buffer_alias_metadata_suffix(scope)
        ));
    }
    ir.push_str("  ret void\n}\n");
    let ctx = inkwell::context::Context::create();
    let module = crate::inprocess::parse_ir_text(&ctx, &ir, "alias_scopes")
        .unwrap_or_else(|e| panic!("LLVM rejected the alias tables: {e:#}"));
    module
        .verify()
        .unwrap_or_else(|e| panic!("{}", e.to_string()));
}

#[test]
fn loop_ids_follow_the_alias_tables_from_the_same_counter() {
    let (mut llmod, scopes) = module_with_scopes(&[120]);
    let after = llmod.allocate_metadata_id();
    let last = *scopes.iter().max().unwrap();
    // scope ids, then their lists, then the domain: all below the next id.
    assert!(
        after > last + 2,
        "counter {after} is not past scope !{last}'s lists"
    );
}
