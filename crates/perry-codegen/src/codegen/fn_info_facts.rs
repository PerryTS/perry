//! Recording a module's `JsFunctionInfo` facts.

use super::*;

/// The codegen metadata a module's `JsFunctionInfo` facts come from
/// (`crate::fn_info`), keyed by closure func-id or wrapper symbol.
pub(super) struct FnInfoFactSources<'a> {
    pub(super) closure_rest_params: &'a HashMap<u32, usize>,
    pub(super) closure_arities: &'a HashMap<u32, u32>,
    pub(super) closure_lengths: &'a HashMap<u32, u32>,
    pub(super) closure_arrow_functions: &'a std::collections::HashSet<u32>,
    pub(super) trusted_box_closures:
        &'a std::collections::HashMap<u32, crate::codegen::closure_collect::TrustedBoxClosure>,
    pub(super) versioned_loop_callbacks: &'a std::collections::HashSet<u32>,
    pub(super) user_fn_wrapper_rest: &'a [(String, usize)],
    pub(super) closure_synthetic_arguments: &'a std::collections::HashSet<u32>,
    pub(super) user_fn_wrapper_synthetic_arguments: &'a std::collections::HashSet<String>,
    pub(super) closure_rest_and_arguments: &'a std::collections::HashSet<u32>,
    pub(super) user_fn_wrapper_rest_and_arguments: &'a std::collections::HashSet<String>,
    pub(super) user_fn_wrapper_arity: &'a [(String, u32)],
    pub(super) user_fn_wrapper_length: &'a [(String, u32)],
    pub(super) user_fn_wrapper_async: &'a std::collections::HashSet<String>,
    pub(super) user_fn_wrapper_generator: &'a std::collections::HashSet<String>,
    pub(super) user_fn_wrapper_async_generator: &'a std::collections::HashSet<String>,
    pub(super) user_fn_wrapper_strict: &'a std::collections::HashSet<String>,
}

/// Record every fact the module knows about its own closure bodies and
/// value wrappers in their `JsFunctionInfo`s (`crate::fn_info`):
///
/// * rest kind + fixed arity (#493, #653; #915's synthesized `arguments`
///   bundles ALL args; rest-and-arguments passes both arrays) — dynamic
///   dispatch bundles trailing args for call sites that cannot see the
///   body's signature (`obj.cb(a, b, c)` with `cb` a `(...args) => …` field);
/// * declared arity of non-rest bodies (#421) — `fn.length`'s fallback;
/// * ECMAScript `.length` (default-aware; Ramda's `converge` / `juxt` read
///   it from function values);
/// * arrow, strict (#4850 OrdinaryCallBindThis), async, generator and
///   async-generator (#3664) bits;
/// * the trusted direct-call and versioned-loop clones of eligible arrows.
pub(super) fn record_fn_info_facts(
    llmod: &LlModule,
    module_prefix: &str,
    src: FnInfoFactSources<'_>,
) {
    use crate::fn_info::RestKind;
    let closure = |fid: u32| format!("perry_closure_{}__{}", module_prefix, fid);
    for (&fid, &fixed) in src.closure_rest_params {
        let kind = if src.closure_rest_and_arguments.contains(&fid) {
            RestKind::UserAndArguments
        } else if src.closure_synthetic_arguments.contains(&fid) {
            RestKind::SyntheticArguments
        } else {
            RestKind::User
        };
        llmod.note_fn_info(&closure(fid), |f| f.set_rest(fixed, kind));
    }
    for (&fid, &arity) in src.closure_arities {
        llmod.note_fn_info(&closure(fid), |f| f.set_declared(arity));
    }
    for (&fid, &length) in src.closure_lengths {
        llmod.note_fn_info(&closure(fid), |f| f.set_length(length));
    }
    for &fid in src.closure_arrow_functions {
        llmod.note_fn_info(&closure(fid), |f| f.set_arrow());
    }
    // The clones hang off arrow bodies only (the runtime's resolvers take
    // them from an arrow's info).
    for (&fid, plan) in src.trusted_box_closures {
        if !src.closure_arrow_functions.contains(&fid) {
            continue;
        }
        let public = closure(fid);
        let trusted = format!("{public}$trusted_boxes");
        let versioned = src
            .versioned_loop_callbacks
            .contains(&fid)
            .then(|| format!("{trusted}$versioned_loop"));
        llmod.note_fn_info(&public, |f| {
            f.trusted = Some(crate::fn_info::CloneTarget {
                symbol: trusted,
                captures: plan.capture_count,
                boxed_mask: plan.boxed_capture_mask,
            });
            f.versioned = versioned.map(|symbol| crate::fn_info::CloneTarget {
                symbol,
                captures: plan.capture_count,
                boxed_mask: plan.boxed_capture_mask,
            });
        });
    }
    let rest_wrappers: std::collections::HashSet<&str> = src
        .user_fn_wrapper_rest
        .iter()
        .map(|(s, _)| s.as_str())
        .collect();
    for (wrap_sym, fixed) in src.user_fn_wrapper_rest {
        let kind = if src.user_fn_wrapper_rest_and_arguments.contains(wrap_sym) {
            RestKind::UserAndArguments
        } else if src.user_fn_wrapper_synthetic_arguments.contains(wrap_sym) {
            RestKind::SyntheticArguments
        } else {
            RestKind::User
        };
        llmod.note_fn_info(wrap_sym, |f| f.set_rest(*fixed, kind));
    }
    for (wrap_sym, arity) in src.user_fn_wrapper_arity {
        if rest_wrappers.contains(wrap_sym.as_str()) {
            continue;
        }
        llmod.note_fn_info(wrap_sym, |f| f.set_declared(*arity));
    }
    for (wrap_sym, length) in src.user_fn_wrapper_length {
        llmod.note_fn_info(wrap_sym, |f| f.set_length(*length));
    }
    for wrap_sym in src.user_fn_wrapper_async {
        llmod.note_fn_info(wrap_sym, |f| f.set_async());
    }
    for wrap_sym in src.user_fn_wrapper_generator {
        llmod.note_fn_info(wrap_sym, |f| f.set_generator());
    }
    for wrap_sym in src.user_fn_wrapper_async_generator {
        llmod.note_fn_info(wrap_sym, |f| f.set_async_generator());
    }
    for wrap_sym in src.user_fn_wrapper_strict {
        llmod.note_fn_info(wrap_sym, |f| f.set_strict());
    }
}
