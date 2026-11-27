//! Rule NOT-ERROR: the false side of `x instanceof Error`, the right-hand
//! side the bare global name holding the intrinsic constructor
//! (`Cons::NotError`), never holds a value whose prototype chain reaches
//! `%Error.prototype%`.
//!
//! InstanceofOperator (ECMA-262 13.10.2) runs `GetMethod(Error,
//! @@hasInstance)`; with no program-installed one it finds
//! `%Function.prototype%[@@hasInstance]` (non-writable, non-configurable,
//! 20.2.3.6), which is OrdinaryHasInstance (7.3.22): true for every object
//! whose chain reaches `Error.prototype` (non-writable, non-configurable,
//! 20.5.2.3). The compiled program answers the same (perry-runtime
//! `instanceof/static_dispatch.rs`: a native Error by its brand, an object
//! by its recorded prototype or its class chain to `CLASS_ID_ERROR`). The
//! filter drops a value while
//! - it is a native Error (`native_error_tok`), or an instance of a program
//!   class whose static `extends` chain (`TmplInfo::parent`) ends at a
//!   built-in `Error` or NativeError (`native_parent`); an `extends`
//!   expression with no static class (`parent_value`) drops nothing;
//! - no code outside the program may hold it, no hard operation marks it
//!   and no [[Prototype]] write may reach it (`super_relink`: a
//!   `setPrototypeOf`, a `__proto__` store), and the same for each class
//!   prototype on that chain and a NativeError's prototype;
//! - `hook` (what `Error[@@hasInstance]` may find) holds no value but the
//!   intrinsic `%Function.prototype%[@@hasInstance]` (a program-installed
//!   hook is another value), and the `Error` constructor itself is not
//!   marked hard or relinked.
//!
//! Once one of them fails, `settle_not_error` passes the token (monotone:
//! a released token stays released).
use crate::model::*;

/// The constructors whose `prototype` has `%Error.prototype%` on its
/// chain: `Error` and the NativeErrors (20.5.5, 20.5.6), `AggregateError`
/// (20.5.7).
const ERROR_CTORS: &[&str] = &[
    "Error",
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "AggregateError",
];

/// The realm declaration name of `%Function.prototype%[@@hasInstance]`
/// (perry-runtime `object/global_this/typed_array.rs`, its `#[realm]`
/// export).
const FUNCTION_HAS_INSTANCE: &str = "Function.prototype[\"@@hasInstance\"]";

impl Model {
    /// No outside code, hard operation or [[Prototype]] write reaches `t`.
    fn not_error_intact(&self, t: Tok) -> bool {
        !self.is_escaped(t) && !self.hard.contains_key(&t) && !self.super_relink.contains(&t)
    }

    /// The intrinsic `%Function.prototype%[@@hasInstance]` (20.2.3.6), what
    /// `GetMethod(Error, @@hasInstance)` finds when no program installed
    /// one: OrdinaryHasInstance. It is non-writable and non-configurable,
    /// so the slot holds the realm's own method.
    fn is_function_has_instance(&self, t: Tok) -> bool {
        matches!(
            self.toks[t as usize],
            TokKind::BuiltinFn(Native::Member {
                identity: FUNCTION_HAS_INSTANCE,
                holder: None,
            })
        )
    }

    /// Does the false side of `instanceof Error` never see `t` now?
    pub(crate) fn not_error_drops(&mut self, t: Tok, hook: NodeId) -> bool {
        if t == TOP || self.is_marker(t) || !self.not_error_intact(t) {
            return false;
        }
        if !self.nodes[hook as usize]
            .pts
            .to_sorted()
            .iter()
            .all(|&h| self.is_function_has_instance(h))
        {
            return false;
        }
        let ctor = self.tok(TokKind::Builtin(BuiltinNs::Error));
        if self.hard.contains_key(&ctor) || self.super_relink.contains(&ctor) {
            return false;
        }
        if self.native_error_tok(t) {
            return true;
        }
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            return false;
        };
        if !matches!(self.tmpls[tm as usize].kind, TmplKind::Class { .. }) {
            return false;
        }
        // The chain the instance is born with (`proto_node`): its class's
        // prototype, then each `extends` parent's.
        let mut q = self.constructor_origin(tm);
        // A chain longer than any the program declares is no proof.
        for _ in 0..self.tmpls.len() {
            // A prototype no value holds yet has no token, nor any mark.
            let p = self.tok_index.get(&TokKind::Proto(q)).copied();
            if p.is_some_and(|p| !self.not_error_intact(p)) {
                return false;
            }
            let info = &self.tmpls[q as usize];
            match (info.parent, info.native_parent.clone()) {
                (Some(parent), _) => q = self.constructor_origin(parent),
                (None, Some((module, name))) => {
                    if module != "builtin" || !ERROR_CTORS.contains(&name.as_str()) {
                        return false;
                    }
                    if name == "Error" {
                        return true;
                    }
                    let np = crate::builtins::native_proto(self, &name);
                    return self.not_error_intact(np);
                }
                (None, None) => return false,
            }
        }
        false
    }

    /// Once the worklist is empty: pass every held token that no longer
    /// qualifies (`not_error_drops`).
    pub(crate) fn settle_not_error(&mut self) -> bool {
        if self.not_error_held.is_empty() {
            return false;
        }
        let held = std::mem::take(&mut self.not_error_held);
        let mut kept = Vec::with_capacity(held.len());
        for (t, hook, dst) in held {
            if self.not_error_drops(t, hook) {
                kept.push((t, hook, dst));
            } else {
                self.add_tok(dst, t);
            }
        }
        self.not_error_held = kept;
        !self.wl.is_empty()
    }
}
