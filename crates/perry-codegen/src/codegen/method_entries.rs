//! The closure-convention entries of class methods: the code a class's own
//! function objects run (`<body>__clo` for a static method, `<method>__eclo`
//! for an instance method on the prototype). Split out of `string_pool.rs`,
//! whose class-registration loop emits them, for the file-size gate.

use super::InitChunker;
use crate::types::{DOUBLE, I32, I64};

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
    {
        let n = e.param_count as usize;
        let mut params: Vec<(crate::types::LlvmType, String)> =
            vec![(I64, "%callee".to_string()), (I64, "%this".to_string())];
        params.extend((0..n).map(|i| (DOUBLE, format!("%a{}", i))));
        let f = chunker
            .module()
            .define_function(&entry_name, DOUBLE, params);
        let _ = f.create_block("entry");
        let b = f.block_mut(0).unwrap();
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
        let call_args: Vec<(crate::types::LlvmType, &str)> =
            arg_names.iter().map(|a| (DOUBLE, a.as_str())).collect();
        let r = b.call(DOUBLE, &e.llvm_name, &call_args);
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
    {
        let n = e.param_count as usize;
        let mut params: Vec<(crate::types::LlvmType, String)> =
            vec![(I64, "%callee".to_string()), (I64, "%this".to_string())];
        params.extend((0..n).map(|i| (DOUBLE, format!("%a{}", i))));
        let f = chunker
            .module()
            .define_function(&entry_name, DOUBLE, params);
        let _ = f.create_block("entry");
        let b = f.block_mut(0).unwrap();
        let depth = b.call(
            I64,
            "js_class_method_entry_enter_home",
            &[(I32, &e.cid.to_string()), (I64, "%this"), (I64, "%callee")],
        );
        let this_box = b.bitcast_i64_to_double("%this");
        let arg_names: Vec<String> = (0..n).map(|i| format!("%a{}", i)).collect();
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(DOUBLE, this_box.as_str())];
        call_args.extend(arg_names.iter().map(|a| (DOUBLE, a.as_str())));
        let r = b.call(DOUBLE, &e.llvm_name, &call_args);
        b.call_void("js_class_method_entry_leave", &[(I64, &depth)]);
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
        f.set_non_constructor();
    });
    let info_ref = chunker.current_block().fn_info_ref(&entry_name);
    (format!("@{}", entry_name), info_ref)
}
