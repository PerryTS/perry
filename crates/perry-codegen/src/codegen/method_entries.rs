//! The closure-convention entries of class methods: the code a class's own
//! function objects run (`<body>__clo` for a static method, `<method>__eclo`
//! for an instance method on the prototype). Split out of `string_pool.rs`,
//! whose class-registration loop emits them, for the file-size gate.

use super::InitChunker;
use crate::types::{DOUBLE, I32, I64, PTR};

/// A ClassBody static method's closure-convention entry (`<body>__clo`).
pub(super) struct StaticMethodEntry {
    pub(super) cid: u32,
    pub(super) llvm_name: String,
    pub(super) param_count: u32,
    pub(super) spec_length: u32,
    pub(super) has_user_rest: bool,
    pub(super) has_synth_args: bool,
    /// The class is a per-evaluation template: each evaluation's function
    /// object for this method is at home in that evaluation's class object.
    pub(super) home: bool,
    pub(super) generator_owner: bool,
    pub(super) is_generator: bool,
}

/// Define `<body>__clo(callee, this, args...)`, the code of a ClassBody static
/// method's own function object: the call's `this` (the JS body ABI's receiver
/// parameter) becomes the body's `this` (enter), the body runs, leave drops
/// what enter set up. Arity, rest bundling, length and strictness are facts of
/// the entry's own `JsFunctionInfo`, as for any function body (the caller
/// registers the name against the code). Returns `(@<body>__clo,
/// @<body>__clo$info)`: the code and the info a function object allocates
/// from.
pub(super) fn emit_static_method_entry(
    chunker: &mut InitChunker<'_>,
    e: &StaticMethodEntry,
) -> (String, String) {
    use crate::fn_info::RestKind;
    let entry_name = format!("{}__clo", e.llvm_name);
    let _fallback = crate::fallback_diag::emission(
        &entry_name,
        "lexical home",
        "static method entry",
        "static method entry requires selecting/restoring its lexical class and this binding",
        "class identity, lexical home and static this binding",
        &[e.cid],
    );
    {
        let n = e.param_count as usize;
        let mut params: Vec<(crate::types::LlvmType, String)> =
            vec![(I64, "%callee".to_string()), (I64, "%this".to_string())];
        params.extend((0..n).map(|i| (DOUBLE, format!("%a{}", i))));
        let f = chunker
            .module()
            .define_function(&entry_name, DOUBLE, params);
        if e.generator_owner {
            f.enable_shadow_frame(2);
        }
        let _ = f.create_block("entry");
        let b = f.block_mut(0).unwrap();
        let roots = if e.generator_owner {
            let owner = b.alloca(I64);
            b.store(I64, "%callee", &owner);
            b.call_void("js_shadow_slot_bind", &[(I32, "0"), (PTR, &owner)]);
            let recv = b.alloca(DOUBLE);
            let this = b.bitcast_i64_to_double("%this");
            b.store(DOUBLE, &this, &recv);
            b.call_void("js_shadow_slot_bind", &[(I32, "1"), (PTR, &recv)]);
            Some((owner, recv))
        } else {
            None
        };
        if e.home {
            b.call_void(
                "js_static_method_entry_enter_home",
                &[(I32, &e.cid.to_string()), (I64, "%this"), (I64, "%callee")],
            );
        } else {
            b.call_void(
                "js_static_method_entry_enter",
                &[(I32, &e.cid.to_string()), (I64, "%this")],
            );
        }
        let arg_names: Vec<String> = (0..n).map(|i| format!("%a{}", i)).collect();
        let mut call_args: Vec<(crate::types::LlvmType, &str)> =
            arg_names.iter().map(|a| (DOUBLE, a.as_str())).collect();
        let owner_arg;
        let target = if e.generator_owner {
            let owner = b.load(I64, &roots.as_ref().unwrap().0);
            // The backing operand must outlive the call-argument borrow below.
            owner_arg = owner;
            call_args.insert(0, (I64, owner_arg.as_str()));
            crate::codegen::helpers::generator_method_owner_name(&e.llvm_name)
        } else {
            e.llvm_name.clone()
        };
        let r = b.call(DOUBLE, &target, &call_args);
        b.call_void("js_static_method_entry_leave", &[]);
        b.ret(DOUBLE, &r);
    }
    let (rest, rest_kind) = match (e.has_user_rest, e.has_synth_args) {
        (true, true) => (
            Some(e.param_count.saturating_sub(2)),
            Some(RestKind::UserAndArguments),
        ),
        (true, false) => (Some(e.param_count.saturating_sub(1)), Some(RestKind::User)),
        (false, true) => (
            Some(e.param_count.saturating_sub(1)),
            Some(RestKind::SyntheticArguments),
        ),
        (false, false) => (None, None),
    };
    chunker.module().note_fn_info(&entry_name, |f| {
        match (rest, rest_kind) {
            (Some(fixed), Some(kind)) => f.set_rest(fixed as usize, kind),
            _ => f.set_declared(e.param_count),
        }
        f.set_length(e.spec_length);
        f.set_strict();
        if e.is_generator {
            f.set_generator();
        }
    });
    let info_ref = chunker.current_block().fn_info_ref(&entry_name);
    (format!("@{}", entry_name), info_ref)
}

/// Define `<method>__eclo(callee, this, args...)`, the code of the function
/// object a class's prototype holds for an instance method: the call's `this`
/// is the body's receiver, and the body runs in the evaluation the function
/// object belongs to (its home class object, the object's one capture; a
/// declared class's object has no home and runs in its receiver's) for its
/// private names and captured environment. Arity, rest bundling, length,
/// strictness and being no constructor are facts of the entry's own
/// `JsFunctionInfo`. Returns `(@<method>__eclo, @<method>__eclo$info)`.
pub(super) fn emit_class_method_entry(
    chunker: &mut InitChunker<'_>,
    e: &StaticMethodEntry,
) -> (String, String) {
    use crate::fn_info::RestKind;
    let entry_name = format!("{}__eclo", e.llvm_name);
    let _fallback = crate::fallback_diag::emission(
        &entry_name,
        "lexical home",
        "method entry",
        "per-evaluation method entry requires selecting/restoring its lexical home and private brand",
        "per-evaluation class identity and private lexical brand",
        &[e.cid],
    );
    {
        let n = e.param_count as usize;
        let mut params: Vec<(crate::types::LlvmType, String)> =
            vec![(I64, "%callee".to_string()), (I64, "%this".to_string())];
        params.extend((0..n).map(|i| (DOUBLE, format!("%a{}", i))));
        let f = chunker
            .module()
            .define_function(&entry_name, DOUBLE, params);
        if e.generator_owner {
            f.enable_shadow_frame(2);
        }
        let _ = f.create_block("entry");
        let b = f.block_mut(0).unwrap();
        let roots = if e.generator_owner {
            let owner = b.alloca(I64);
            b.store(I64, "%callee", &owner);
            b.call_void("js_shadow_slot_bind", &[(I32, "0"), (PTR, &owner)]);
            let recv = b.alloca(DOUBLE);
            let this = b.bitcast_i64_to_double("%this");
            b.store(DOUBLE, &this, &recv);
            b.call_void("js_shadow_slot_bind", &[(I32, "1"), (PTR, &recv)]);
            Some((owner, recv))
        } else {
            None
        };
        // A declaration's function object has no evaluation capture. Its
        // instance shapes already carry the private brand, and its body reads
        // the explicit receiver. Only a per-evaluation method needs to select
        // the lexical home independently of that receiver.
        let depth = e.home.then(|| {
            b.call(
                I64,
                "js_class_method_entry_enter_home",
                &[(I32, &e.cid.to_string()), (I64, "%this"), (I64, "%callee")],
            )
        });
        let this_box = if let Some((_, recv)) = &roots {
            b.load(DOUBLE, recv)
        } else {
            b.bitcast_i64_to_double("%this")
        };
        let arg_names: Vec<String> = (0..n).map(|i| format!("%a{}", i)).collect();
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(DOUBLE, this_box.as_str())];
        call_args.extend(arg_names.iter().map(|a| (DOUBLE, a.as_str())));
        let owner_arg;
        let target = if e.generator_owner {
            let owner = b.load(I64, &roots.as_ref().unwrap().0);
            // The backing operand must outlive the call-argument borrow below.
            owner_arg = owner;
            call_args.insert(0, (I64, owner_arg.as_str()));
            crate::codegen::helpers::generator_method_owner_name(&e.llvm_name)
        } else {
            e.llvm_name.clone()
        };
        let r = b.call(DOUBLE, &target, &call_args);
        if let Some(depth) = depth {
            b.call_void("js_class_method_entry_leave", &[(I64, &depth)]);
        }
        b.ret(DOUBLE, &r);
    }
    let (rest, rest_kind) = match (e.has_user_rest, e.has_synth_args) {
        (true, true) => (
            Some(e.param_count.saturating_sub(2)),
            Some(RestKind::UserAndArguments),
        ),
        (true, false) => (Some(e.param_count.saturating_sub(1)), Some(RestKind::User)),
        (false, true) => (
            Some(e.param_count.saturating_sub(1)),
            Some(RestKind::SyntheticArguments),
        ),
        (false, false) => (None, None),
    };
    chunker.module().note_fn_info(&entry_name, |f| {
        match (rest, rest_kind) {
            (Some(fixed), Some(kind)) => f.set_rest(fixed as usize, kind),
            _ => f.set_declared(e.param_count),
        }
        f.set_length(e.spec_length);
        f.set_strict();
        if e.is_generator {
            f.set_generator();
        }
        f.set_non_constructor();
    });
    let info_ref = chunker.current_block().fn_info_ref(&entry_name);
    (format!("@{}", entry_name), info_ref)
}

/// Source flags of a class member, without another persistent metadata table.
pub(super) fn source_needs_owner(
    classes: &[(u32, &perry_hir::Class)],
    cid: u32,
    id: u32,
    fresh: bool,
) -> bool {
    classes
        .iter()
        .find(|(c, _)| *c == cid)
        .is_some_and(|(_, class)| {
            class
                .methods
                .iter()
                .chain(class.static_methods.iter())
                .chain(class.computed_members.iter().map(|m| &m.function))
                .any(|f| {
                    f.id == id
                        && crate::codegen::helpers::body_needs_explicit_method_owner(&f.body, fresh)
                })
        })
}

pub(super) fn source_is_generator(classes: &[(u32, &perry_hir::Class)], cid: u32, id: u32) -> bool {
    classes
        .iter()
        .find(|(c, _)| *c == cid)
        .is_some_and(|(_, class)| {
            class
                .methods
                .iter()
                .chain(class.static_methods.iter())
                .chain(class.computed_members.iter().map(|m| &m.function))
                .any(|f| {
                    f.id == id && crate::codegen::helpers::body_has_fixed_generator_state(&f.body)
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(home: bool) -> String {
        let mut module = crate::module::LlModule::new("x86_64-unknown-linux-gnu");
        module.declare_function("method_body", DOUBLE, &[DOUBLE, DOUBLE]);
        let mut chunker = InitChunker::new(&mut module, "init".into(), 100);
        chunker.roll_if_full();
        emit_class_method_entry(
            &mut chunker,
            &StaticMethodEntry {
                cid: 801,
                llvm_name: "method_body".into(),
                param_count: 1,
                spec_length: 1,
                has_user_rest: false,
                has_synth_args: false,
                home,
                generator_owner: false,
                is_generator: false,
            },
        );
        chunker.finish();
        module.to_ir()
    }

    #[test]
    fn declaration_super_home_shares_the_direct_body_but_fresh_home_needs_owner() {
        let body = vec![perry_hir::Stmt::Let {
            id: 1,
            name: "home".into(),
            ty: perry_hir::types::Type::Any,
            mutable: false,
            init: Some(perry_hir::Expr::ClassMethodHome {
                class_name: "C".into(),
                is_static: false,
            }),
        }];
        assert!(crate::codegen::helpers::body_needs_method_owner(&body));
        assert!(!crate::codegen::helpers::body_needs_explicit_method_owner(
            &body, false
        ));
        assert!(crate::codegen::helpers::body_needs_explicit_method_owner(
            &body, true
        ));
    }

    #[test]
    fn declaration_entries_forward_the_receiver_without_a_home_scope() {
        let ir = entry(false);
        assert!(ir.contains("define double @method_body__eclo"));
        assert!(ir.contains("call double @method_body("));
        assert!(!ir.contains("@js_class_method_entry_enter_home"));
        assert!(!ir.contains("@js_class_method_entry_leave"));
    }

    #[test]
    fn evaluation_entries_keep_their_lexical_home() {
        let ir = entry(true);
        assert!(ir.contains("call i64 @js_class_method_entry_enter_home"));
        assert!(ir.contains("call void @js_class_method_entry_leave"));
    }
}

/// Computed instance generators need a callable entry as named ones do.
pub(super) fn emit_computed_generator_entries(
    chunker: &mut InitChunker<'_>,
    classes: &[(u32, &perry_hir::Class)],
    fresh: &std::collections::HashSet<u32>,
    prefix: &str,
) {
    for &(cid, class) in classes {
        for m in class.computed_members.iter().filter(|m| {
            !m.is_static
                && crate::codegen::helpers::body_needs_explicit_method_owner(
                    &m.function.body,
                    fresh.contains(&cid),
                )
        }) {
            let f = &m.function;
            let llvm_name = format!(
                "perry_method_{}__{}__{}",
                prefix,
                crate::codegen::helpers::sanitize_member(&class.name),
                crate::codegen::helpers::sanitize_member(&f.name)
            );
            emit_class_method_entry(
                chunker,
                &StaticMethodEntry {
                    cid,
                    llvm_name,
                    param_count: f.params.len() as u32,
                    spec_length: crate::codegen::spec_function_length(&f.params) as u32,
                    has_user_rest: f
                        .params
                        .iter()
                        .any(|p| p.is_rest && p.arguments_object.is_none()),
                    has_synth_args: f.params.iter().any(|p| p.arguments_object.is_some()),
                    home: fresh.contains(&cid),
                    generator_owner: true,
                    is_generator: crate::codegen::helpers::body_has_fixed_generator_state(&f.body),
                },
            );
        }
    }
}
