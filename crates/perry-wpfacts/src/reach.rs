//! Switches of the activation-reach rules: each names a call edge or a
//! lookup the solve makes so that a body the program can enter is never
//! called inactive (`BodyReach::inactive`). Off restores the solve without
//! the rule.

use crate::model::{Cons, Key, Model, NodeId};

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

/// `PERRY_FACTS_REACH_NEW_TARGET=0`: `Reflect.construct(T, args, N)` whose
/// result the model holds as unknown (a built-in `T`, a function `N`)
/// leaves what `N.prototype` holds where it is
/// (`Model::new_target_unknown`).
pub(crate) fn new_target() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_NEW_TARGET").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}
/// `PERRY_FACTS_REACH_STEP_DONE=0`: an iteration consumer reads only the
/// value of a step result, not its `done` first (`Model::step_done`).
pub(crate) fn step_done() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_STEP_DONE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

impl Model {
    /// IteratorComplete (ECMA-262 7.4.5) on the step results `result`
    /// holds: every consumer of the iteration protocol (a for-of step,
    /// array destructuring, spread, `yield*`, `Array.from` and the other
    /// built-in consumers) reads `done` of what `next()` returned before
    /// it reads the value, a [[Get]] like any other: a getter there runs.
    /// The value read is ToBoolean'd by the runtime and goes nowhere; the
    /// node returned holds it (`None`: the rule is off).
    pub(crate) fn step_done(&mut self, result: NodeId) -> Option<NodeId> {
        if !self.reach_step_done {
            return None;
        }
        let done = self.atom("done");
        let dst = self.node_l("step-done");
        self.nodes[dst as usize].frame = self.nodes[result as usize].frame;
        let key = Key::Named(done);
        self.cons(result, Cons::Load { key, dst });
        Some(dst)
    }
}
