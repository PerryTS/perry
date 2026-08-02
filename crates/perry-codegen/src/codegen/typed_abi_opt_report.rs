//! `--opt-report` (#6952) rendering for specialized-ABI rejections.
//!
//! Split out of `typed_abi.rs` for the 2000-line file-size gate. This is an
//! inherent impl on `TypedCloneRejectionReason`, so it can live in any module
//! of the crate; the variant list itself stays next to the decision logic.

use super::typed_abi::TypedCloneRejectionReason;

impl TypedCloneRejectionReason {
    /// `--opt-report` (#6952) rendering of a **specialized-ABI entry**
    /// rejection: a human expansion of the variant plus who can act on it.
    ///
    /// Extends this existing vocabulary rather than paralleling it — the
    /// variant IS the rule, so the report cites `as_str()` as the rule name
    /// and this as the explanation. Only the `Spec*` and shared-precondition
    /// variants that the spec-ABI decision can actually produce are given
    /// bespoke text; the rest fall through to a generic line.
    pub(crate) fn opt_report_reason(
        self,
    ) -> (&'static str, crate::opt_report::Tier, Option<&'static str>) {
        use crate::opt_report::Tier;
        match self {
            Self::SpecTupleUnproven => (
                "no call site proved a representation tuple for this \
                 function's parameters, so every call uses the boxed ABI and \
                 re-unboxes on entry. Either the arguments are genuinely \
                 polymorphic, or their types are outside what the specialized \
                 ABI covers today (i32/u32, f64, bool, string, typed-array \
                 pointer) — object, array and union parameters are not \
                 covered yet, so a correct annotation does not always help.",
                Tier::CompilerLimitation,
                None,
            ),
            Self::SpecBudgetExceeded => (
                "the module-wide specialized-entry budget \
                 (PERRY_SPECIALIZED_ABI_MAX) was exhausted before this \
                 function's turn; it routes to the boxed entry.",
                Tier::CompilerLimitation,
                None,
            ),
            Self::SpecTypedCloneOverlap => (
                "the function already carries a typed_abi clone family, which \
                 is mutually exclusive with a specialized entry in this phase.",
                Tier::CompilerLimitation,
                None,
            ),
            Self::SpecReadsDynamicThis => (
                "the callee reads dynamic `this`, so direct call sites must \
                 juggle the implicit-this slot and cannot take the raw ABI.",
                Tier::CompilerLimitation,
                None,
            ),
            Self::AsyncOrGenerator => (
                "async and generator bodies route their locals through shared \
                 cells (the async-to-generator transform), which the \
                 specialized ABI must not touch.",
                Tier::CompilerLimitation,
                None,
            ),
            Self::Captures | Self::CapturesThis | Self::CapturesNewTarget => (
                "the function captures from an enclosing scope; captures are \
                 passed through the closure environment, not the raw ABI.",
                Tier::Fixable,
                None,
            ),
            Self::ParamDefault => (
                "a parameter has a default value, so the callee cannot assume \
                 a fixed argument count on the raw entry.",
                Tier::Fixable,
                None,
            ),
            Self::RestParam => (
                "a rest parameter makes the argument count dynamic, so there \
                 is no fixed representation tuple.",
                Tier::Fixable,
                None,
            ),
            Self::ArgumentsObject => (
                "the body reads `arguments`, which requires the boxed \
                 argument vector.",
                Tier::Fixable,
                None,
            ),
            _ => (
                "the specialized-ABI precondition named by the rule did not \
                 hold; the function keeps the boxed entry ABI.",
                Tier::CompilerLimitation,
                None,
            ),
        }
    }
}
