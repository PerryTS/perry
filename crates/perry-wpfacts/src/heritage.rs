//! Evaluated class heritage uses the ordinary value/import graph. This is
//! analysis state only; runtime layout/identity guards remain authoritative.

use crate::gen::{Fx, Gen};
use crate::model::*;

impl Gen<'_> {
    pub(crate) fn evaluated_parent(&mut self, mi: u32, name: &str, value: NodeId) {
        if let Some(tm) = self.resolve_class_name(mi, name) {
            if let Some(parent) = self.m.tmpls[tm as usize].evaluated_parent {
                self.m.edge(value, parent);
                return;
            }
        }
        // Metadata without an evaluated-parent slot cannot prove heritage.
        self.to_sink(value, "E11 evaluated parent without a class template");
    }

    pub(crate) fn super_access(&mut self, fx: &Fx, key: Key, act: ChainAct) {
        self.super_access_of(fx.class, fx.this, key, act);
    }

    pub(crate) fn super_access_of(
        &mut self,
        class: Option<TmplId>,
        recv: NodeId,
        key: Key,
        act: ChainAct,
    ) {
        if let Some(tm) = class {
            if self.m.tmpls[tm as usize].evaluated_parent.is_some() {
                // Both forms are conservatively modeled, as for named super.
                for holder in [TokKind::Proto(tm), TokKind::ClassObj(tm)] {
                    let holder = self.m.tok(holder);
                    let parents = self.m.proto_node(holder);
                    self.m.cons(parents, Cons::SuperFrom { recv, key, act });
                }
                return;
            }
        }
        let starts = match class {
            Some(tm) => self.super_starts_of(tm),
            None => vec![TOP],
        };
        for start in starts {
            self.m.cons(recv, Cons::Super { start, key, act });
        }
    }
}

/// An evaluated ancestor has no single proven birth width. Keep value and
/// method flow, but leave layout/slot admission to the existing guarded code.
pub(crate) fn evaluated_chain(m: &Model, mut tm: TmplId) -> bool {
    loop {
        let t = &m.tmpls[tm as usize];
        if t.evaluated_parent.is_some() {
            return true;
        }
        match t.parent {
            Some(p) => tm = p,
            None => return false,
        }
    }
}

impl Model {
    pub(crate) fn apply_class_parent(
        &mut self,
        class: TmplId,
        reason: u32,
        toks: &[Tok],
        prim: u16,
    ) {
        let ctor = self.tok(TokKind::ClassObj(class));
        let proto = self.tok(TokKind::Proto(class));
        let ctor_parent = self.proto_node(ctor);
        let proto_parent = self.proto_node(proto);
        let prototype = self.atom("prototype");
        for &t in toks {
            match self.toks[t as usize] {
                TokKind::ClassObj(_) => {
                    self.add_tok(ctor_parent, t);
                    // The evaluated constructor's *actual* prototype value:
                    // writes through the existing field graph remain visible.
                    let parent_proto = self.field(t, prototype);
                    self.edge(parent_proto, proto_parent);
                }
                _ => {
                    self.escape(t, reason);
                    let inst = self.tok(TokKind::Obj(class));
                    self.escape(inst, reason);
                    self.mark_hard(inst, Hard::E11ReflectConstruct, reason);
                    self.add_tok(ctor_parent, TOP);
                    self.add_tok(proto_parent, TOP);
                }
            }
        }
        // `extends null` has a null instance-prototype parent and the ordinary
        // Function.prototype constructor chain (collapsed to ObjectProto).
        if prim & prim::NULL != 0 {
            let op = self.tok(TokKind::ObjectProto);
            self.add_tok(ctor_parent, op);
            self.add_prim(proto_parent, prim::NULL);
        }
        // Other primitive heritage throws before any instance can be born.
    }

    pub(crate) fn apply_parent_ctor(&mut self, call: CallId, this: Tok, toks: &[Tok]) {
        for &t in toks {
            match self.toks[t as usize] {
                TokKind::ClassObj(parent) => self.run_ctor_chain(call, parent, this),
                _ => {
                    // Unknown parent code sees the instance and arguments; its
                    // object return may replace the instance.
                    let c = self.calls[call as usize].clone();
                    let r = self.reason("E11 invocation of an unknown evaluated parent");
                    self.escape(this, r);
                    for arg in c.args.iter().chain(&c.spread) {
                        self.sink_node(*arg, r);
                    }
                    self.add_tok(c.ret, TOP);
                }
            }
        }
    }
}
