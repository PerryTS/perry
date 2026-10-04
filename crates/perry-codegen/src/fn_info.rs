//! One static `JsFunctionInfo` (perry-abi) per JS body a function object runs.
//!
//! A function object's header points at its body's info
//! (`perry_abi::CLOSURE_INFO_OFFSET`); every fact a caller needs about the
//! body — its parameter count, rest kind, `.length`, arrow / strict / async /
//! generator bits, the compiler-private direct-call clones — is read from it.
//! Codegen emits the info as a constant next to the body it describes,
//! `@<body>$info`, and every allocation of a function object names that
//! constant instead of the body.
//!
//! Emission is two-sided and module-scoped:
//! * every allocation site asks its block for the info of the body it
//!   allocates (`LlBlock::fn_info_ref`), which records the request;
//! * the module records the facts it knows about its own bodies
//!   ([`FnInfoState::facts_mut`]) — the same metadata that used to become
//!   module-init registration calls;
//! * after all functions exist, [`FnInfoState::render_globals`] defines one
//!   info per body this module DEFINES that is requested here or visible to
//!   other modules (an external-linkage value wrapper another module may
//!   allocate), and declares `external` the info of every requested body
//!   another module defines. A foreign body's info is never copied: the
//!   runtime keys a function's singleton object by its info, so every module
//!   must name the one the defining module emits.

use std::collections::{BTreeMap, BTreeSet};

use crate::runtime_abi::{
    FN_ARROW, FN_ASYNC, FN_ASYNC_GENERATOR, FN_COMPILED_BODY, FN_GENERATOR, FN_HAS_DECLARED,
    FN_HAS_LENGTH, FN_NON_CONSTRUCTOR, FN_PERMANENT_IMAGE, FN_REST_SYNTHETIC_ARGUMENTS,
    FN_REST_USER, FN_REST_USER_AND_ARGUMENTS, FN_STRICT,
};

/// The LLVM type of a `JsFunctionInfo`, field for field (perry-abi's
/// `#[repr(C)]` layout; LLVM lays it out for the target exactly as the
/// runtime's C layout does, 32-bit pointers included).
pub(crate) const INFO_TYPE: &str =
    "{ ptr, i16, i16, i32, i32, i32, ptr, i64, ptr, i32, i16, i16, i64 }";

/// The info symbol of `body` (no `@`).
pub(crate) fn info_symbol(body: &str) -> String {
    format!("{body}$info")
}

/// Shared by closure lowering and the static final-shape pre-pass.
pub(crate) fn closure_body_symbol(module_prefix: &str, func_id: u32) -> String {
    format!("perry_closure_{module_prefix}__{func_id}")
}

/// A compiler-private direct-call clone of a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CloneTarget {
    pub symbol: String,
    pub captures: u32,
    pub boxed_mask: u64,
}

/// What the module knows about one of its bodies beyond its parameter
/// count, which comes from the body's definition.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FnInfoFacts {
    pub flags: u32,
    pub rest_fixed: u16,
    pub length: u32,
    pub declared: u16,
    pub trusted: Option<CloneTarget>,
    pub versioned: Option<CloneTarget>,
}

/// The rest kind of a body with a rest / `arguments` array.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RestKind {
    User,
    SyntheticArguments,
    UserAndArguments,
}

impl FnInfoFacts {
    /// A body whose array parameter bundles every argument from `fixed` on.
    pub(crate) fn set_rest(&mut self, fixed: usize, kind: RestKind) {
        let bit = match kind {
            RestKind::User => FN_REST_USER,
            RestKind::SyntheticArguments => FN_REST_SYNTHETIC_ARGUMENTS,
            RestKind::UserAndArguments => FN_REST_USER_AND_ARGUMENTS,
        };
        self.flags = (self.flags
            & !(FN_REST_USER | FN_REST_SYNTHETIC_ARGUMENTS | FN_REST_USER_AND_ARGUMENTS))
            | bit;
        self.rest_fixed = saturate_u16(fixed as u64);
    }

    /// The JS-visible declared parameter count (`.length`'s fallback).
    pub(crate) fn set_declared(&mut self, declared: u32) {
        self.declared = saturate_u16(u64::from(declared));
        self.flags |= FN_HAS_DECLARED;
    }

    /// The ECMAScript `.length`.
    pub(crate) fn set_length(&mut self, length: u32) {
        self.length = length;
        self.flags |= FN_HAS_LENGTH;
    }

    pub(crate) fn set_arrow(&mut self) {
        self.flags |= FN_ARROW;
    }

    pub(crate) fn set_strict(&mut self) {
        self.flags |= FN_STRICT;
    }

    /// A method: no `[[Construct]]` and no own `prototype`.
    pub(crate) fn set_non_constructor(&mut self) {
        self.flags |= FN_NON_CONSTRUCTOR;
    }

    pub(crate) fn set_async(&mut self) {
        self.flags |= FN_ASYNC;
    }

    pub(crate) fn set_generator(&mut self) {
        self.flags |= FN_GENERATOR;
    }

    /// `async function*`: async and async-generator both.
    pub(crate) fn set_async_generator(&mut self) {
        self.flags |= FN_ASYNC_GENERATOR | FN_ASYNC;
    }
}

fn saturate_u16(value: u64) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// A body this module defines, as the info renderer sees it.
pub(crate) struct DefinedBody {
    /// JS `f64` parameters after the callee and the receiver.
    pub params: usize,
    /// The body's LLVM linkage (empty = external).
    pub linkage: String,
}

/// The module's info requests and facts.
#[derive(Default)]
pub(crate) struct FnInfoState {
    requested: BTreeSet<String>,
    facts: BTreeMap<String, FnInfoFacts>,
    /// Bodies whose info address a separate static-seed object will name.
    static_seed_bodies: BTreeSet<String>,
}

impl FnInfoState {
    /// Record that code in this module names `body`'s info; returns the
    /// `@`-prefixed info symbol.
    pub(crate) fn request(&mut self, body: &str) -> String {
        if !self.requested.contains(body) {
            self.requested.insert(body.to_string());
        }
        format!("@{}", info_symbol(body))
    }

    /// Reserve a stable body-info symbol for a future static seed unit.
    /// This also requests the info definition. The body remains local; only
    /// its info becomes linkable when the definer renders this module.
    /// No birth collector calls this until the seed ABI has module-init parity.
    pub(crate) fn request_static_seed_body(&mut self, body: &str) -> String {
        self.static_seed_bodies.insert(body.to_string());
        self.request(body)
    }

    /// The facts of a body this module defines.
    pub(crate) fn facts_mut(&mut self, body: &str) -> &mut FnInfoFacts {
        self.facts.entry(body.to_string()).or_default()
    }

    /// The `@...$info` global lines: a definition for every body this module
    /// defines that is requested or has recorded facts or is `exported`
    /// (an external value wrapper another module may allocate), and an
    /// `external` declaration for every requested body defined elsewhere.
    /// `defined(body)` answers for this module's definitions.
    pub(crate) fn render_globals(
        &self,
        defined: impl Fn(&str) -> Option<DefinedBody>,
        exported: impl IntoIterator<Item = String>,
        permanent_image: bool,
    ) -> Vec<String> {
        let mut bodies: BTreeSet<&str> = self.requested.iter().map(String::as_str).collect();
        bodies.extend(self.facts.keys().map(String::as_str));
        let exported: Vec<String> = exported.into_iter().collect();
        bodies.extend(exported.iter().map(String::as_str));
        let mut out = Vec::with_capacity(bodies.len());
        for body in bodies {
            match defined(body) {
                Some(def) => out.push(render_definition(
                    body,
                    &def,
                    self.facts.get(body).cloned().unwrap_or_default(),
                    permanent_image,
                    self.static_seed_bodies.contains(body),
                )),
                None if self.requested.contains(body) => out.push(format!(
                    "@{} = external constant {}",
                    info_symbol(body),
                    INFO_TYPE
                )),
                // Facts about a body this module does not define (a closure
                // the module never emitted) describe nothing to emit.
                None => {}
            }
        }
        out
    }
}

fn render_definition(
    body: &str,
    def: &DefinedBody,
    facts: FnInfoFacts,
    permanent_image: bool,
    static_seed: bool,
) -> String {
    let linkage = if static_seed {
        "hidden ".to_string()
    } else {
        match def.linkage.as_str() {
            "" => String::new(),
            other => format!("{other} "),
        }
    };
    let clone = |target: &Option<CloneTarget>| match target {
        Some(t) => (format!("@{}", t.symbol), t.captures, t.boxed_mask),
        None => ("null".to_string(), 0, 0),
    };
    let (trusted_code, trusted_captures, trusted_mask) = clone(&facts.trusted);
    let (versioned_code, versioned_captures, versioned_mask) = clone(&facts.versioned);
    format!(
        "@{info} = {linkage}constant {ty} {{ ptr @{body}, i16 {params}, i16 {rest}, i32 {flags}, \
         i32 {length}, i32 {tcap}, ptr {tcode}, i64 {tmask}, ptr {vcode}, i32 {vcap}, \
         i16 {declared}, i16 0, i64 {vmask} }}",
        info = info_symbol(body),
        ty = INFO_TYPE,
        params = saturate_u16(def.params as u64),
        rest = facts.rest_fixed,
        // Every body this renders is compiled source (`FN_COMPILED_BODY`).
        flags = facts.flags
            | FN_COMPILED_BODY
            | if permanent_image {
                FN_PERMANENT_IMAGE
            } else {
                0
            },
        length = facts.length,
        tcap = trusted_captures,
        tcode = trusted_code,
        tmask = trusted_mask as i64,
        vcode = versioned_code,
        vcap = versioned_captures,
        declared = facts.declared,
        vmask = versioned_mask as i64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defined(params: usize, linkage: &str) -> Option<DefinedBody> {
        Some(DefinedBody {
            params,
            linkage: linkage.to_string(),
        })
    }

    #[test]
    fn a_requested_local_body_gets_a_definition_with_its_facts() {
        let mut state = FnInfoState::default();
        assert_eq!(
            state.request("perry_closure_m__3"),
            "@perry_closure_m__3$info"
        );
        let facts = state.facts_mut("perry_closure_m__3");
        facts.set_rest(1, RestKind::User);
        facts.set_length(1);
        facts.set_arrow();
        let lines = state.render_globals(
            |b| {
                (b == "perry_closure_m__3")
                    .then(|| defined(2, "internal"))
                    .flatten()
            },
            [],
            false,
        );
        assert_eq!(
            lines,
            vec![format!(
                "@perry_closure_m__3$info = internal constant {INFO_TYPE} {{ ptr @perry_closure_m__3, \
                 i16 2, i16 1, i32 {}, i32 1, i32 0, ptr null, i64 0, ptr null, i32 0, i16 0, i16 0, i64 0 }}",
                FN_REST_USER | FN_HAS_LENGTH | FN_ARROW | FN_COMPILED_BODY
            )]
        );
    }

    #[test]
    fn only_a_permanent_image_marks_defined_body_infos() {
        let mut state = FnInfoState::default();
        state.request("perry_closure_m__3");
        let transient = state.render_globals(|_| defined(0, "internal"), [], false);
        let permanent = state.render_globals(|_| defined(0, "internal"), [], true);
        assert!(transient[0].contains(&format!("i32 {FN_COMPILED_BODY}, i32 0")));
        assert!(permanent[0].contains(&format!(
            "i32 {}, i32 0",
            FN_PERMANENT_IMAGE | FN_COMPILED_BODY
        )));
    }

    #[test]
    fn seed_info_has_linkable_stable_symbol_but_body_keeps_local_linkage() {
        let mut state = FnInfoState::default();
        let body = "perry_closure_m__3";
        assert_eq!(
            state.request_static_seed_body(body),
            format!("@{}", info_symbol(body))
        );
        assert_eq!(
            state.request_static_seed_body(body),
            format!("@{}", info_symbol(body))
        );
        let lines = state.render_globals(|_| defined(0, "internal"), [], true);
        assert_eq!(
            lines.len(),
            1,
            "one body has one info despite fresh closures"
        );
        assert!(lines[0].starts_with(&format!("@{} = hidden constant", info_symbol(body))));
        assert!(lines[0].contains(&format!("ptr @{body}")));
        assert!(lines[0].contains(&format!("i32 {}", FN_PERMANENT_IMAGE | FN_COMPILED_BODY)));
    }

    #[test]
    fn foreign_seed_info_is_only_declared_by_importer() {
        let mut state = FnInfoState::default();
        let body = "perry_closure_other__3";
        state.request_static_seed_body(body);
        assert_eq!(
            state.render_globals(|_| None, [], true),
            vec![format!(
                "@{} = external constant {INFO_TYPE}",
                info_symbol(body)
            )]
        );
    }

    #[test]
    fn a_foreign_body_is_declared_never_copied() {
        let mut state = FnInfoState::default();
        state.request("__perry_wrap_perry_fn_other__f");
        let lines = state.render_globals(|_| None, [], false);
        assert_eq!(
            lines,
            vec![format!(
                "@__perry_wrap_perry_fn_other__f$info = external constant {INFO_TYPE}"
            )]
        );
    }

    #[test]
    fn an_exported_wrapper_is_defined_even_when_nothing_here_allocates_it() {
        let state = FnInfoState::default();
        let lines = state.render_globals(
            |b| {
                (b == "__perry_wrap_perry_fn_m__g")
                    .then(|| defined(1, ""))
                    .flatten()
            },
            ["__perry_wrap_perry_fn_m__g".to_string()],
            false,
        );
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with(&format!(
            "@__perry_wrap_perry_fn_m__g$info = constant {INFO_TYPE} {{ ptr @__perry_wrap_perry_fn_m__g, i16 1,"
        )));
    }

    #[test]
    fn clone_targets_are_named() {
        let mut state = FnInfoState::default();
        state.request("perry_closure_m__9");
        state.facts_mut("perry_closure_m__9").trusted = Some(CloneTarget {
            symbol: "perry_closure_m__9$trusted_boxes".to_string(),
            captures: 2,
            boxed_mask: 0b10,
        });
        let lines = state.render_globals(|_| defined(0, "internal"), [], false);
        assert!(
            lines[0].contains("i32 2, ptr @perry_closure_m__9$trusted_boxes, i64 2, ptr null"),
            "{}",
            lines[0]
        );
    }
}
