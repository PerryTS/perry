//! Switches of the activation-reach rules: each names a call edge or a
//! lookup the solve makes so that a body the program can enter is never
//! called inactive (`BodyReach::inactive`). Off restores the solve without
//! the rule.

use crate::model::{CallId, CallSite, Cons, Key, Model, NodeId};

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

/// `PERRY_FACTS_REACH_SPREAD_ARGS=0`: a hand-written built-in rule reads
/// its arguments by position among the direct arguments only; what a
/// spread argument supplies reaches no position (`Model::positional`).
pub(crate) fn spread_args() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_SPREAD_ARGS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_FACTS_REACH_ITER_CLOSE=0`: a consumer that can stop before its
/// iterator is done (`Array.from` with a mapper, a collection constructor,
/// `Object.fromEntries`, `groupBy`, `Promise.all` ...) never calls the
/// iterator's `return` (`Gen::iterate_closed`, `builtins::iterate`).
pub(crate) fn iter_close() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_ITER_CLOSE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

impl Model {
    /// The call site of `call` as a hand-written built-in rule reads it by
    /// position (`c.args.first()`, `c.args.get(i)`): its first `n`
    /// positions at least. Without a spread argument, the site itself. A
    /// spread shifts positions and they are not kept (`f(...a, b)` and
    /// `f(b, ...a)` are both `args [b]`, `spread [a]`), so position `i`
    /// holds a direct argument at index `i` or before, or an element a
    /// spread argument supplies: the rule the declared effects use
    /// (`effects::Run::root`, `RealmSlotRoot::Arg`). A callback given as
    /// `xs.forEach(...[cb])` is then the callback the rule calls. `spread`
    /// stays, so a rule can tell that a position may also be absent.
    pub(crate) fn positional(&mut self, call: CallId, n: usize) -> CallSite {
        let mut c = self.calls[call as usize].clone();
        if c.spread.is_empty() || !self.reach_spread_args {
            return c;
        }
        let frame = self.call_run(call);
        let elems = self.node_l("spread-arg-elems");
        self.nodes[elems as usize].frame = frame;
        let key = self.elem_key();
        for &s in &c.spread {
            self.cons(s, Cons::Load { key, dst: elems });
        }
        let direct = std::mem::take(&mut c.args);
        for i in 0..n.max(direct.len()) {
            let a = self.node_l("spread-arg");
            self.nodes[a as usize].frame = frame;
            for &d in direct.iter().take(i + 1) {
                self.edge(d, a);
            }
            self.edge(elems, a);
            c.args.push(a);
        }
        c
    }

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
