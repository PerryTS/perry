//! Representation-selection env knobs, and the per-context flags derived from
//! them (#7128).
//!
//! ## Why the knobs need a module of their own
//!
//! Perry ships one bisection knob per unboxed representation. Their whole
//! value is A/B: build twice, flip one knob, attribute the difference to that
//! representation. **That attribution is only sound if the knob moves nothing
//! else** — and twice it did not, in ways that were invisible from the knob's
//! name and that retroactively weakened every measurement taken through it:
//!
//! * **`PERRY_CANONICAL_I32_LOCALS=0` also disabled every `Ptr<Shape>`
//!   consumption.** Phase 5a reused `repsel_context_allows_canonical_i32` as
//!   *its* context gate. #7121 split the `FnCtx` FIELD, but left the four
//!   ordinary-body construction sites initialising both fields from one
//!   `repsel_allows` bool whose first conjunct was the canonical-i32 env read
//!   — so the field split was a no-op for the knob. Census under that knob
//!   read `ptr-shape: 7 selected, 0 consumed`, with all six consumption sites
//!   printing `NEVER FIRES`. Two of the four workloads whose object moves
//!   under the knob (`batch`, `suite_09_method_calls`) were therefore
//!   measuring two representations at once.
//! * **`PERRY_CANONICAL_STR_LOCALS=0` also disabled three lowerings that never
//!   consult a selected `Str` local at all** — the inline `StringRef` retag
//!   (`native_value/materialize.rs`), the proven-heap operand arm of
//!   `str_operand_handle_tag_dispatched` (`lower_string_method.rs`), and the
//!   tag-dispatched `.length` on any statically-string expression
//!   (`expr/property_get.rs`). All three key on a value's static string TYPE,
//!   not on slot selection, so 24 of the 26 census workloads emitted
//!   differently under the knob — including workloads whose `canonical-str`
//!   count is zero.
//!
//! ## The rule
//!
//! **A knob may move only the sites of the representation it names.** Stated
//! mechanically, and checked by
//! `scripts/compiler_output_harness/repsel_knob_isolation.py` over the census
//! corpus:
//!
//! 1. with knob `X=0` and everything else default, no census key outside `X`'s
//!    own keys may change; and
//! 2. a workload in which `X` promotes nothing must emit a **byte-identical**
//!    object.
//!
//! ## The knob table
//!
//! | env knob | representation | census keys |
//! |---|---|---|
//! | `PERRY_CANONICAL_I32_LOCALS` | canonical `i32`/`u32` slots (Phase 1) | `canonical-i32`, `canonical-u32` |
//! | `PERRY_CANONICAL_STR_LOCALS` | canonical `Str` slots (Phase 3a) | `canonical-str` |
//! | `PERRY_PTR_SHAPE_LOCALS` | `Ptr<Shape>` (Phase 3b/5a) | `ptr-shape`, `ptr-shape-consumed` |
//! | `PERRY_PTR_NUMARRAY_LOCALS` | `Ptr<NumArray>` (Phase 4a.3) | `ptr-numarray` |
//! | `PERRY_INT_VALUED_LOCALS` | int-valued TA residency (#6898) | `int-valued-ta` |
//! | `PERRY_SPECIALIZED_ABI` | specialized ABI entries (Phase 2) | `spec-abi-entry`, `spec-abi-taptr-slot` |
//! | `PERRY_STATIC_STRING_LOWERING` | *(not a representation)* string fast paths keyed on a value's static type | — |
//!
//! The last row is the residue of defect 2. Those three lowerings are real and
//! shipped, but they are not representation selection: nothing about them
//! depends on a local having been selected. They keep a kill switch of their
//! own rather than riding on `Str`'s, so that `PERRY_CANONICAL_STR_LOCALS`
//! means what it says. `PERRY_PTR_SHAPE_THIS` is a sub-knob of
//! `PERRY_PTR_SHAPE_LOCALS` (Phase 5a's proven-`this` method clones only) and is
//! honoured by the same collector.

use super::slot_rep::{
    body_context_denial, canonical_i32_locals_enabled, canonical_str_locals_enabled,
    MODULE_INIT_CONTEXT,
};

/// `PERRY_STATIC_STRING_LOWERING` gate. Enabled by default; `=0`/`off`/`false`
/// reverts the three string fast paths that key on a value's **static string
/// type** rather than on a canonical-`Str` slot selection:
///
/// * `native_value::materialize::nanbox_string_ref_boxed` — the inline
///   `or STRING_TAG; bitcast` retag of a raw `StringRef` handle, with the
///   null-handle case kept in a cold `js_nanbox_string` arm;
/// * `lower_string_method::str_operand_handle_tag_dispatched`'s
///   `proven_heap_string_operand` arm — inline `bitcast; and POINTER_MASK` for
///   a literal / `String(x)` operand;
/// * `expr::property_get`'s tag-dispatched `.length` on any `is_string_expr`
///   receiver.
///
/// All three shipped in Phase 3a (#6909) behind `PERRY_CANONICAL_STR_LOCALS`,
/// which made that knob unusable for A/B: they fire on programs that select no
/// canonical-`Str` local at all (#7128, finding B). Splitting them out here is
/// **behaviour-preserving by construction** — both knobs default on, so the
/// default build emits the identical bytes; only the `=0` arms change meaning.
/// Keyed into the object cache (`object_cache.rs`).
pub(crate) fn static_string_lowering_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_STATIC_STRING_LOWERING").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The env knobs that gate a *context* decision, read once and passed by value.
///
/// Passed explicitly rather than read inside [`RepselContextFlags::derive`] for
/// one reason: the readers are process-wide `OnceLock`s, so a unit test cannot
/// flip them. With the gates as a parameter, `derive` is a pure function and
/// the "one knob moves one flag" property is testable directly — which is what
/// the tests at the bottom of this file do, and what nothing checked before
/// #7128.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RepselGates {
    pub canonical_i32: bool,
    pub canonical_str: bool,
    pub ptr_shape: bool,
}

impl RepselGates {
    pub(crate) fn from_env() -> Self {
        Self {
            canonical_i32: canonical_i32_locals_enabled(),
            canonical_str: canonical_str_locals_enabled(),
            ptr_shape: crate::collectors::ptr_shape_locals_enabled(),
        }
    }
}

/// Which kind of body a `FnCtx` is being constructed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepselBody {
    /// An ordinary function, method or closure body. `is_generator` /
    /// `was_plain_async` carry the closure spellings too
    /// (`local_generator_funcs`, `async_step_closures`) — same conjunction,
    /// different registry.
    Body {
        is_async: bool,
        is_generator: bool,
        was_plain_async: bool,
    },
    /// A module-init or program-entry body (`codegen/entry.rs`). Canonical
    /// i32/u32/Str are allowed here since #7109; `Ptr<Shape>` is not — see
    /// [`MODULE_INIT_CONTEXT`] for the audit and #6991 for the live rooting bug
    /// that keeps it off.
    Entry,
}

/// The three context gates a `FnCtx` carries, plus the rule names the
/// `--opt-report` denial records use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RepselContextFlags {
    pub allows_canonical_i32: bool,
    pub allows_canonical_str: bool,
    pub allows_ptr_shape: bool,
    /// Why this context forbids canonical (i32/u32/Str) selection, or `None`.
    /// Structural reasons only — never the env knobs (see
    /// `slot_rep::body_context_denial`).
    pub canonical_denial: Option<&'static str>,
    /// Why this context forbids acting on a `Ptr<Shape>` proof, or `None`.
    pub ptr_shape_denial: Option<&'static str>,
}

impl RepselContextFlags {
    /// Pure derivation: `gates` in, flags out, no env reads.
    ///
    /// **Each output flag reads exactly one input gate.** That is the whole
    /// point of this function existing, and `each_knob_moves_exactly_one_flag`
    /// below is the assertion. Before #7128 the four ordinary-body `FnCtx`
    /// construction sites computed `allows_ptr_shape` from the canonical-i32
    /// gate, so `PERRY_CANONICAL_I32_LOCALS=0` silently turned off a second
    /// representation.
    pub(crate) fn derive(gates: RepselGates, body: RepselBody) -> Self {
        match body {
            RepselBody::Body {
                is_async,
                is_generator,
                was_plain_async,
            } => {
                let denial = body_context_denial(is_async, is_generator, was_plain_async);
                let structural_ok = denial.is_none();
                Self {
                    allows_canonical_i32: gates.canonical_i32 && structural_ok,
                    allows_canonical_str: gates.canonical_str && structural_ok,
                    allows_ptr_shape: gates.ptr_shape && structural_ok,
                    canonical_denial: denial,
                    // Same structural rule, its own field: before #7109 this
                    // read `repsel_context_denial`, which no longer names a
                    // rule in entry bodies.
                    ptr_shape_denial: denial,
                }
            }
            RepselBody::Entry => Self {
                allows_canonical_i32: gates.canonical_i32,
                allows_canonical_str: gates.canonical_str,
                // Unconditionally off, regardless of `gates.ptr_shape`: the
                // exclusion is structural (#6991), not a knob. Written as a
                // literal so a future reader cannot mistake it for something
                // `PERRY_PTR_SHAPE_LOCALS=1` could turn back on.
                allows_ptr_shape: false,
                canonical_denial: None,
                ptr_shape_denial: Some(MODULE_INIT_CONTEXT),
            },
        }
    }

    /// Read the env gates and derive for an ordinary body.
    pub(crate) fn for_body(is_async: bool, is_generator: bool, was_plain_async: bool) -> Self {
        Self::derive(
            RepselGates::from_env(),
            RepselBody::Body {
                is_async,
                is_generator,
                was_plain_async,
            },
        )
    }

    /// Read the env gates and derive for a module-init / program-entry body.
    pub(crate) fn for_entry() -> Self {
        Self::derive(RepselGates::from_env(), RepselBody::Entry)
    }

    /// Whether the value-level screens should be collected purely so
    /// `--opt-report` can count the structural denial exactly (#7106).
    pub(crate) fn report_denial(&self) -> bool {
        super::slot_rep::report_context_denial(self.canonical_denial)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ON: RepselGates = RepselGates {
        canonical_i32: true,
        canonical_str: true,
        ptr_shape: true,
    };

    const SYNC: RepselBody = RepselBody::Body {
        is_async: false,
        is_generator: false,
        was_plain_async: false,
    };

    fn allows(flags: &RepselContextFlags) -> (bool, bool, bool) {
        (
            flags.allows_canonical_i32,
            flags.allows_canonical_str,
            flags.allows_ptr_shape,
        )
    }

    /// #7128's acceptance property, at the smallest scale it can be stated:
    /// turning off exactly one knob turns off exactly one context flag.
    ///
    /// Sabotage-checked: restoring the pre-#7128 coupling
    /// (`allows_ptr_shape: gates.canonical_i32 && structural_ok`) takes the
    /// `canonical_i32` row red with "knob canonical_i32 also moved
    /// allows_ptr_shape", which is precisely finding A.
    #[test]
    fn each_knob_moves_exactly_one_flag() {
        let baseline = RepselContextFlags::derive(ALL_ON, SYNC);
        assert_eq!(allows(&baseline), (true, true, true));

        let cases: [(&str, RepselGates, (bool, bool, bool)); 3] = [
            (
                "canonical_i32",
                RepselGates {
                    canonical_i32: false,
                    ..ALL_ON
                },
                (false, true, true),
            ),
            (
                "canonical_str",
                RepselGates {
                    canonical_str: false,
                    ..ALL_ON
                },
                (true, false, true),
            ),
            (
                "ptr_shape",
                RepselGates {
                    ptr_shape: false,
                    ..ALL_ON
                },
                (true, true, false),
            ),
        ];
        for (name, gates, want) in cases {
            let got = RepselContextFlags::derive(gates, SYNC);
            assert_eq!(
                allows(&got),
                want,
                "knob {name} moved a flag it does not own: \
                 (canonical_i32, canonical_str, ptr_shape) = {:?}, expected {want:?}",
                allows(&got)
            );
            // A bisection knob is not a structural rule: it must never invent a
            // `--opt-report` denial the default build cannot emit.
            assert_eq!(got.canonical_denial, None, "knob {name}");
            assert_eq!(got.ptr_shape_denial, None, "knob {name}");
        }
    }

    /// The same property for the entry context, where `Ptr<Shape>` is off for a
    /// structural reason: the two canonical knobs must still move only
    /// themselves, and the `Ptr<Shape>` knob must move nothing (it is already
    /// off).
    #[test]
    fn entry_context_keeps_ptr_shape_off_and_names_the_rule() {
        let entry = RepselContextFlags::derive(ALL_ON, RepselBody::Entry);
        assert_eq!(allows(&entry), (true, true, false));
        assert_eq!(entry.canonical_denial, None);
        assert_eq!(entry.ptr_shape_denial, Some(MODULE_INIT_CONTEXT));

        for gates in [
            RepselGates {
                canonical_i32: false,
                ..ALL_ON
            },
            RepselGates {
                canonical_str: false,
                ..ALL_ON
            },
            RepselGates {
                ptr_shape: false,
                ..ALL_ON
            },
        ] {
            let got = RepselContextFlags::derive(gates, RepselBody::Entry);
            assert!(!got.allows_ptr_shape);
            assert_eq!(got.ptr_shape_denial, Some(MODULE_INIT_CONTEXT));
            assert_eq!(got.allows_canonical_i32, gates.canonical_i32);
            assert_eq!(got.allows_canonical_str, gates.canonical_str);
        }
    }

    /// The structural denials still apply to all three representations, and
    /// still name a rule. This is the part #7128 must NOT change: an async body
    /// is excluded because the async-to-generator transform boxes body locals,
    /// which is a hazard, not a knob.
    #[test]
    fn structural_denials_apply_to_every_representation() {
        for (body, rule) in [
            (
                RepselBody::Body {
                    is_async: true,
                    is_generator: false,
                    was_plain_async: false,
                },
                "async_body",
            ),
            (
                RepselBody::Body {
                    is_async: false,
                    is_generator: true,
                    was_plain_async: false,
                },
                "generator_body",
            ),
            (
                RepselBody::Body {
                    is_async: false,
                    is_generator: false,
                    was_plain_async: true,
                },
                "was_plain_async_body",
            ),
        ] {
            let got = RepselContextFlags::derive(ALL_ON, body);
            assert_eq!(allows(&got), (false, false, false), "{rule}");
            assert_eq!(got.canonical_denial, Some(rule));
            assert_eq!(got.ptr_shape_denial, Some(rule));
        }
    }
}
