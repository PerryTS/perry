//! The model-node constructors the exhaustive `expr_rest` match calls: the
//! primitive-protocol touches (`prim_kids`, `primbox_kids`), the container-node
//! builders (`bobj`, `new_arr_from`, `new_iter_from`), and the element/callback
//! flows (`elem_store`, `elem_load`, `callback`, `array_cb`). The match itself
//! stays in `gen_rest`: routing is one exhaustive table, not a per-group guess.
use super::*;

impl<'a> Gen<'a> {
    pub(super) fn typed_array_new(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;

        // Constructor algorithm: source iterable/array-like reads,
        // followed by numeric element conversion, not escape.
        let args: Vec<_> = kids(e).iter().map(|k| self.expr(fx, k)).collect();
        let (call, _) = self.new_call(ThisArg::Undef, args.clone(), vec![]);
        let (t, n) = self.bobj_view(mi, "TypedArray");
        // Its buffer: an ArrayBuffer argument of the constructor (23.2.5.1.3),
        // else the one it allocates; `from` always copies (23.2.2.1).
        let (ab, _) = self.bobj_view(mi, "ArrayBuffer");
        match args.first() {
            Some(&v) if !matches!(e, Expr::Uint8ArrayFrom(..)) => self.m.cons(
                v,
                Cons::ViewedBuffer {
                    view: t,
                    fresh: Some(ab),
                    share: false,
                },
            ),
            _ => {
                let f = crate::builtins::viewed_buffer(&mut self.m, t);
                self.m.add_tok(f, ab);
            }
        }
        let owner = self.begin_indexed_owner(mi, t, e);
        if let Some(&v) = args.first() {
            let vals = crate::builtins::typed_array_methods::source(&mut self.m, call, v);
            crate::builtins::string_methods::to_prim(
                &mut self.m,
                vals,
                "typed-array constructor element ToNumber/ToBigInt",
            );
            self.m.cons(v, Cons::ShareBuffer { ta: t });
            if owner {
                if let TokKind::Obj(tm, _) = self.m.toks[t as usize] {
                    self.m.ta_birth_input.insert(tm, v);
                }
            }
        }
        let fa = self.m.field_any(t);
        self.m.add_prim(fa, prim::NUM | prim::BIGINT);
        n
    }

    /// The first HIR child is the receiver; the remaining children are
    /// arguments. Solve normal property lookup, including replacements
    /// and unknown receiver families, before choosing a builtin rule.
    pub(super) fn receiver_method(&mut self, fx: &Fx, e: &Expr, name: &str) -> NodeId {
        let ks = kids(e);
        let recv = self.expr(fx, ks[0]);
        let args = ks[1..].iter().map(|x| self.expr(fx, x)).collect();
        let a = self.m.atom(name);
        self.mcall(recv, Key::Named(a), args, vec![])
    }

    /// A folded builtin-method node: `receiver.method(...args)`, resolved
    /// through the receiver like any method call.
    pub(super) fn folded_call(
        &mut self,
        fx: &Fx,
        call: &perry_hir::folded_call::FoldedCall<'_>,
    ) -> NodeId {
        let recv = match call.receiver {
            perry_hir::folded_call::Receiver::Expr(x) => self.expr(fx, x),
            perry_hir::folded_call::Receiver::Local(id) => self.local(fx.module, id),
        };
        let args: Vec<NodeId> = call.args.iter().map(|x| self.expr(fx, x)).collect();
        if call.method == "push" {
            self.list_push(recv, Some(args.len() as u32));
        }
        let a = self.m.atom(call.method);
        self.folded_site(fx, &call.receiver, a, recv);
        self.mcall(recv, Key::Named(a), args, vec![])
    }

    /// Record the site of a folded method node (`method_sites`): the call
    /// `recv.key(..)`, named by its receiver as codegen names it.
    pub(crate) fn folded_site(
        &mut self,
        fx: &Fx,
        receiver: &perry_hir::folded_call::Receiver<'_>,
        key: Atom,
        recv: NodeId,
    ) {
        if let Some(site) = perry_hir::folded_call::receiver_site(receiver) {
            self.folded_sites.push(crate::gen::FoldedSite {
                module: fx.module,
                site,
                key,
                recv,
            });
        }
    }

    pub(super) fn prim_kids(
        &mut self,
        fx: &Fx,
        e: &Expr,
        result: u16,
        why: &str,
        proto: Protocol,
    ) -> NodeId {
        let ks = kids(e);
        self.prim_op(fx, &ks, result, why, proto)
    }

    /// `string.name(...operands)`: the receiver is ToString'd (the lowering
    /// pins the method to `String.prototype`), then the method runs on a
    /// string with the operands (`builtins::string_methods`).
    pub(super) fn string_method(
        &mut self,
        fx: &Fx,
        string: &Expr,
        name: &str,
        operands: &[&Box<Expr>],
    ) -> NodeId {
        let s = self.expr(fx, string);
        crate::builtins::string_methods::to_prim(&mut self.m, s, "string method receiver ToString");
        // The method classifies its operands (`zero_sign`, `with_native`).
        let args: Vec<NodeId> = operands.iter().map(|o| self.expr_zs(fx, o)).collect();
        let recv = self.prim_node(prim::STR);
        let a = self.m.atom(name);
        self.mcall(recv, Key::Named(a), args, vec![])
    }

    /// A primitive container whose operands were already evaluated and are
    /// coerced by nothing.
    pub(super) fn primbox_kids_none(&mut self, fx: &Fx, why: &str) -> NodeId {
        self.prim_box_op(fx, &[], why, Protocol::OwnKeys)
    }

    pub(super) fn primbox_kids(&mut self, fx: &Fx, e: &Expr, why: &str, proto: Protocol) -> NodeId {
        let ks = kids(e);
        self.prim_box_op(fx, &ks, why, proto)
    }

    pub(crate) fn bobj(&mut self, mi: u32, what: &'static str) -> (Tok, NodeId) {
        let (t, n) = self.bobj_view(mi, what);
        // A view born without a buffer argument allocates its own
        // (23.2.5.1.6 AllocateTypedArrayBuffer): one per birth site.
        if matches!(what, "TypedArray" | "Buffer" | "DataView") {
            let (ab, _) = self.bobj_view(mi, "ArrayBuffer");
            let f = crate::builtins::viewed_buffer(&mut self.m, t);
            self.m.add_tok(f, ab);
        }
        (t, n)
    }

    /// `bobj` of a view whose [[ViewedArrayBuffer]] the caller writes.
    pub(crate) fn bobj_view(&mut self, mi: u32, what: &'static str) -> (Tok, NodeId) {
        let tm = self.new_bobj_template(mi, what);
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let n = self.tok_node(t);
        (t, n)
    }

    pub(crate) fn new_arr_from(&mut self, mi: u32, what: &str, elems: &[NodeId]) -> NodeId {
        self.new_arr_of(mi, what, elems, false)
    }

    /// `new_arr_from` of an array whose birth fills each index below its
    /// length in order (ArrayCreate + CreateDataProperty): it has no hole.
    pub(crate) fn new_dense_arr_from(&mut self, mi: u32, what: &str, elems: &[NodeId]) -> NodeId {
        self.new_arr_of(mi, what, elems, true)
    }

    fn new_arr_of(&mut self, mi: u32, what: &str, elems: &[NodeId], dense: bool) -> NodeId {
        let tm = self.new_arr_birth(mi, what, dense, false);
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let fa = self.m.field_any(t);
        for &e in elems {
            self.m.edge(e, fa);
        }
        self.tok_node(t)
    }

    /// A builtin iterator object of kind `what` yielding `vals` through
    /// `next().value`
    /// (`builtins::iterator_method`).
    pub(crate) fn new_iter_from(&mut self, mi: u32, what: &'static str, vals: &[NodeId]) -> NodeId {
        let (t, n) = self.bobj(mi, what);
        let a = self.m.atom("#elems");
        let e = self.m.field(t, a);
        for &v in vals {
            self.m.edge(v, e);
        }
        n
    }

    /// Store `v` into every element/entry of what `recv` holds.
    pub(super) fn elem_store(&mut self, fx: &Fx, recv: NodeId, v: NodeId, what: &str) {
        let ek = self.m.elem_key();
        self.store(fx, recv, ek, v, what.to_string(), None, false);
    }

    /// The fill push `e` (`push_fills`) of fill `fill`: the element store
    /// of `v` into what `recv` holds, at a site marked as that fill's.
    pub(super) fn fill_push(&mut self, fx: &Fx, e: &Expr, recv: NodeId, v: NodeId, fill: u32) {
        let ek = self.m.elem_key();
        let site = self.store_site(fx.module, fx.f, "push".to_string(), None, false);
        self.m.push_fill_sites.insert(site, fill);
        self.m.cons(
            recv,
            Cons::Store {
                key: ek,
                src: v,
                site,
            },
        );
        let offset = match e {
            Expr::NativeMethodCall {
                object: Some(object),
                ..
            } => match object.as_ref() {
                Expr::PropertyGet { byte_offset, .. } => *byte_offset,
                _ => 0,
            },
            _ => 0,
        };
        self.m.push_fill_pushes.push(crate::push_fills::FillPush {
            module: fx.module,
            offset,
            recv,
            fill,
        });
    }

    /// An operation without a rule of its own runs code on `n` once ⊤ is
    /// there (DESIGN SEEDE section 8): ⊤ into the current throw region.
    pub(crate) fn raise_on(&mut self, n: NodeId) {
        let region = self.region();
        self.m.cons(n, Cons::Raise { region });
    }

    /// An async step closure's call: what its body throws settles the
    /// promise, so it runs in the shared cell (rejections; DESIGN SEEDE
    /// section 8), not the caller's region.
    pub(super) fn async_callback(&mut self, cb: NodeId, args: Vec<NodeId>) -> NodeId {
        let region = std::mem::replace(&mut self.m.gen_region, Some(self.thrown));
        let r = self.callback(cb, args);
        self.m.gen_region = region;
        r
    }

    /// Call `cb` with `args` (this = undefined); returns the result node.
    pub(super) fn callback(&mut self, cb: NodeId, args: Vec<NodeId>) -> NodeId {
        self.call_value(cb, ThisArg::Undef, args, vec![])
    }

    /// These operations read numeric elements. The solved receiver decides
    /// whether this is an array-like; other receivers retain full lookup.
    pub(super) fn elem_load(&mut self, recv: NodeId) -> NodeId {
        let key = self.m.elem_key();
        self.load(recv, key)
    }
}
