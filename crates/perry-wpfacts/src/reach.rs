//! Switches of the activation-reach rules: each names a call edge or a
//! lookup the solve makes so that a body the program can enter is never
//! called inactive (`BodyReach::inactive`). Off restores the solve without
//! the rule.

/// `PERRY_FACTS_REACH_FOLDS=0`: a name-keyed fold (`Expr::WeakRefDeref`,
/// `Expr::FinalizationRegistryRegister` / `Unregister`) is its intrinsic
/// only, not also the ordinary method call its runtime helper makes on a
/// receiver of another kind (`Gen::name_fold_call`).
pub(crate) fn name_folds() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_FOLDS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}
