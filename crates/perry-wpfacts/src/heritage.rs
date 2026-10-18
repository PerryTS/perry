//! Evaluated class heritage uses the ordinary value/import graph. This is
//! analysis state only; runtime layout/identity guards remain authoritative.

use crate::gen::{Fx, Gen};
use crate::model::*;

impl Gen<'_> {
    /// Factory specialization can keep an evaluated parent in ClassExprFresh
    /// after resolving/removing the Class metadata's extends_expr. Discover
    /// those sites before prototype nodes, phase analysis, or birth keys exist.
    pub(crate) fn prepare_heritage(&mut self) {
        for (mi, (_, module)) in self.modules.clone().iter().enumerate() {
            let mut names = crate::pts::FxHashSet::default();
            let mut site = |e: &perry_hir::Expr| match e {
                perry_hir::Expr::ClassExprFresh {
                    template,
                    evaluated_parent: Some(_),
                    ..
                } => {
                    names.insert(template.clone());
                }
                perry_hir::Expr::RegisterClassParentDynamic { class_name, .. } => {
                    names.insert(class_name.clone());
                }
                _ => {}
            };
            // The exhaustive visitor includes computed members and closures;
            // the public module visitor additionally covers global initializers.
            perry_hir::tdz_check::for_each_module_expr(module, &mut site);
            perry_hir::for_each_module_expr(module, &mut site);
            for c in &module.classes {
                let tm = self.mods[mi].classes[&c.name];
                let has_site = names.contains(&c.name);
                if has_site {
                    self.evaluated_parent_sites.insert(tm);
                }
                if has_site || c.extends_expr.is_some() {
                    let node = self.m.node_l("evaluated-parent");
                    self.m.tmpls[tm as usize].evaluated_parent = Some(node);
                    let reason = self.m.reason("E11 class with an unknown evaluated parent");
                    self.m.cons(node, Cons::ClassParent { class: tm, reason });
                }
            }
        }
    }

    pub(crate) fn evaluated_parent(&mut self, mi: u32, name: &str, value: NodeId) {
        if let Some(tm) = self.resolve_class_name(mi, name) {
            if let Some(parent) = self.m.tmpls[tm as usize].evaluated_parent {
                self.m.edge(value, parent);
                return;
            }
            let inst = self.m.tok(TokKind::Obj(tm));
            let r = self
                .m
                .reason("E11 evaluated parent without a heritage slot");
            self.m.escape(inst, r);
            self.m.mark_hard(inst, Hard::E11ReflectConstruct, r);
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
    /// An ES class has a non-writable, non-configurable own `prototype`.
    /// Its object can mutate; the constructor's reference cannot be replaced.
    /// Synthetic literal classes and import stubs keep the conservative model.
    pub(crate) fn fixed_class_prototype(&self, t: Tok, key: Key) -> bool {
        let (TokKind::ClassObj(tm), Key::Named(a)) = (&self.toks[t as usize], key) else {
            return false;
        };
        self.atoms[a as usize] == "prototype"
            && matches!(
                &self.tmpls[*tm as usize].kind,
                TmplKind::Class { name, class_id, .. }
                    if *class_id != 0 && !name.starts_with("__AnonShape_")
            )
    }

    pub(crate) fn native_parent_value(&self, class: TmplId, token: Tok) -> Option<String> {
        let name = &self.tmpls[class as usize].native_parent.as_ref()?.1;
        let matches = match self.toks[token as usize] {
            TokKind::Builtin(BuiltinNs::Error) => matches!(
                name.as_str(),
                "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError"
            ),
            TokKind::BuiltinFn => name == "EventEmitter",
            _ => false,
        };
        matches.then(|| name.clone())
    }

    /// Ordinary non-constructor values throw during heritage validation,
    /// before their prototype is read or any child can be constructed.
    /// Keep real class instances conservative: native Function ancestry can
    /// produce callable instances. Opaque functions retain the E11 boundary.
    fn invalid_parent_object(&self, token: Tok) -> bool {
        match self.toks[token as usize] {
            TokKind::Proto(_) | TokKind::ObjectProto | TokKind::PrimBox | TokKind::ModNs(_) => true,
            TokKind::Obj(tm) => match self.tmpls[tm as usize].kind {
                TmplKind::Class { literal_shape, .. } => literal_shape,
                _ => true,
            },
            _ => false,
        }
    }

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
            if self.invalid_parent_object(t) {
                continue;
            }
            if let Some(name) = self.native_parent_value(class, t) {
                self.add_tok(ctor_parent, t);
                let native = crate::builtins::native_proto(self, &name);
                self.add_tok(proto_parent, native);
                continue;
            }
            match self.toks[t as usize] {
                TokKind::ClassObj(_) => {
                    self.add_tok(ctor_parent, t);
                    // The evaluated constructor's *actual* prototype value:
                    // writes through the existing field graph remain visible.
                    let parent_proto = self.field(t, prototype);
                    self.edge(parent_proto, proto_parent);
                }
                _ => {
                    // Once a heritage set includes unknown code, retain the
                    // old whole-value escape boundary. This also covers known
                    // alternatives that may be constructed by that code.
                    let parent = self.tmpls[class as usize].evaluated_parent.unwrap();
                    self.sink_node(parent, reason);
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
}
