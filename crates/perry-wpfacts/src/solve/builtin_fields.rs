//! Rule R1 (STR1 design §3): a store under a key a built-in namespace or
//! native function object does not declare (`builtins::undeclared`) is a
//! field of that object. The runtime never reads such a key, so the value
//! stays the program's: no value is sunk, no gate opens. What can read it
//! besides the program is unknown code that holds the object: code it is
//! handed to (an escape) and interpreted code; for both the object's fields
//! sink, as an escaped program object's do.

use super::*;
use crate::builtins::Undeclared;

/// The computed-key reads of the native function values' shared fields.
const ANY_KEY: Atom = Atom::MAX;

impl Model {
    /// The field node a store under `key` into built-in object `t` writes
    /// (rule R1), or `None`: the store takes the opaque path.
    pub(crate) fn builtin_field(&mut self, t: Tok, key: Key) -> Option<NodeId> {
        let Key::Named(a) = key else {
            return None;
        };
        let holder = match builtins::undeclared(self, t, a)? {
            Undeclared::Namespace => t,
            Undeclared::NativeFn => self.tok(TokKind::BuiltinFn(Native::Any)),
        };
        let n = self.field(holder, a);
        if self.r1_holders.insert(holder) && self.interp_runs {
            self.builtin_fields_sink(holder, None);
        }
        if holder != t {
            self.r1_wake(holder, a, n);
        }
        Some(n)
    }

    /// The reads and calls that waited for the shared native function field
    /// under `a`, and the computed-key ones.
    fn r1_wake(&mut self, any: Tok, a: Atom, n: NodeId) {
        for w in self.r1_waits.remove(&a).unwrap_or_default() {
            self.r1_use(n, w);
        }
        if let Some(ws) = self.r1_waits.remove(&ANY_KEY) {
            let all = self.all_fields(any);
            for w in ws {
                self.r1_use(all, w);
            }
        }
    }

    fn r1_use(&mut self, n: NodeId, w: R1Wait) {
        match w {
            R1Wait::Load(dst) => self.edge(n, dst),
            R1Wait::Call(call, origin) => self.bind_via(n, call, origin),
        }
    }

    /// A read (`R1Wait::Load`) or method call (`R1Wait::Call`) under `key`
    /// through native function value `t` finds what R1 stores put into the
    /// shared field, once one does.
    pub(crate) fn builtin_field_use(&mut self, t: Tok, key: Key, w: R1Wait) {
        let any = match key {
            Key::Named(a) if builtins::native_fn_field(self, t, a) => a,
            Key::Any(_) if builtins::native_fn_holder(self, t) => ANY_KEY,
            _ => return,
        };
        let holder = self.tok(TokKind::BuiltinFn(Native::Any));
        let n = match any {
            ANY_KEY => self.all_fields.get(&holder).copied(),
            a => self.fields.get(&(holder, a)).copied(),
        };
        match n {
            Some(n) => self.r1_use(n, w),
            None => self.r1_waits.entry(any).or_default().push(w),
        }
    }

    /// Built-in object `t` reached unknown code (handed to it, not reached
    /// along a chain: `chain_escape`): what R1 stored on it may be read
    /// there. `None` reason: interpreted code runs.
    pub(crate) fn builtin_fields_sink(&mut self, t: Tok, reason: Option<u32>) {
        if self.unit_mode || t == TOP {
            return;
        }
        let holder = match self.toks[t as usize] {
            TokKind::Builtin(BuiltinNs::GlobalThis) => return,
            TokKind::BuiltinFn(_)
            | TokKind::Builtin(BuiltinNs::ObjectProtoMethod | BuiltinNs::ObjectProtoOwn(_)) => {
                self.tok(TokKind::BuiltinFn(Native::Any))
            }
            TokKind::Builtin(_) => t,
            _ => return,
        };
        let i = if matches!(self.toks[holder as usize], TokKind::BuiltinFn(_)) {
            2
        } else {
            0
        };
        self.r1_sinks[i] += 1;
        if self.r1_holders.contains(&holder) {
            self.r1_sinks[i + 1] += 1;
        }
        let r = match reason {
            Some(r) => r,
            None => self.reason("field of a built-in object interpreted code may read"),
        };
        let all = self.all_fields(holder);
        self.sink_node(all, r);
    }

    /// Interpreted code may run: it reads any built-in object by name.
    pub(crate) fn builtin_fields_interp(&mut self) {
        let mut hs: Vec<Tok> = self.r1_holders.iter().copied().collect();
        hs.sort_unstable();
        for h in hs {
            self.builtin_fields_sink(h, None);
        }
    }
}
