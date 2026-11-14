//! Construction separates the body being run from its allocation's class.
use super::*;

impl Model {
    /// A class object unknown code can construct once it escapes. The class
    /// of a closed object literal (`Class::is_literal_shape`) is a layout,
    /// not a runtime value: no code reaches it as a constructor (the
    /// literal reads `constructor` from `Object.prototype`), so none constructs it.
    pub(crate) fn unknown_constructible(&self, t: Tok) -> bool {
        match self.toks[t as usize] {
            TokKind::ClassObj(tm) => !matches!(
                self.tmpls[tm as usize].kind,
                TmplKind::Class {
                    literal_shape: true,
                    ..
                }
            ),
            _ => false,
        }
    }

    /// The object `Reflect.construct(T, args, new_target)` makes has
    /// `new_target.prototype` as its [[Prototype]]
    /// (OrdinaryCreateFromConstructor, GetPrototypeFromConstructor), and the
    /// model holds that object as ⊤ (a built-in `T`'s own allocation, a
    /// function `new_target` that names no class layout). What the new
    /// target's `prototype` holds is then on the chain of a ⊤ value: it
    /// goes to ⊤ with it, so the members a call on that value finds there
    /// can run.
    fn new_target_unknown(&mut self, call: CallId, new_target: Tok) {
        if !self.reach_new_target || new_target == TOP {
            return;
        }
        let run = self.call_run(call);
        let nt = self.node_l("reflect-new-target");
        self.nodes[nt as usize].frame = run;
        self.add_tok(nt, new_target);
        let protos = self.node_l("reflect-new-target-prototype");
        self.nodes[protos as usize].frame = run;
        let key = Key::Named(self.atom("prototype"));
        self.cons(nt, Cons::Load { key, dst: protos });
        let r = self.reason(
            "Reflect.construct: prototype of the new target of an object the model holds as unknown",
        );
        self.sink_node(protos, r);
    }

    pub(crate) fn reflect_construct(&mut self, call: CallId, target: Tok, new_target: Tok) {
        if !self.sum.ready {
            self.sum
                .deferred
                .push(crate::summary::Deferred::ReflectConstruct(
                    call, target, new_target,
                ));
            return;
        }
        if target == TOP {
            // TOP may denote a retained program constructor. Its actual
            // Reflect call supplies both the allocation and the arguments.
            let targets: Vec<_> = self
                .escaped
                .keys()
                .copied()
                .filter(|&t| self.unknown_constructible(t))
                .collect();
            for t in targets {
                self.reflect_construct(call, t, new_target);
            }
            self.construct(call, TOP);
            return;
        }
        if target == new_target {
            self.construct(call, target);
            return;
        }
        self.diagnostic_callee(call, target);
        let c = self.calls[call as usize].clone();
        if let TokKind::Bound(b) = self.toks[target as usize] {
            // Reuse the existing bound-call expansion boundary. A recursive
            // bound relay cannot merge distinct new targets into ordinary new.
            if self.bound_synth.contains(&call) {
                let r = self.reason(
                    "Reflect.construct through an unknown constructor (recursive bound target)",
                );
                self.escape(target, r);
                self.construct(call, TOP);
                return;
            }
            let (callee, _, mut args) = self.bound[b as usize].clone();
            args.extend(c.args.iter().copied());
            let Some(call2) = self.solver_call_keyed(
                call,
                (crate::summary::MK_BOUND_REFLECT, b, new_target),
                None,
                CallSite {
                    this: ThisArg::Undef,
                    args,
                    spread: c.spread.clone(),
                    ret: c.ret,
                    site: c.site,
                    region: self.thrown,
                },
            ) else {
                return;
            };
            self.bound_synth.insert(call2);
            let nt = self.node_l("bound-construct-new-target");
            self.add_tok(nt, new_target);
            self.cons(
                callee,
                Cons::ReflectNew {
                    call: call2,
                    new_target: nt,
                },
            );
            return;
        }
        // Native/exotic constructors need their own allocation rule. An
        // unknown target cannot establish a nominal allocation class either.
        if !matches!(
            self.toks[target as usize],
            TokKind::ClassObj(_) | TokKind::Func(_, _)
        ) {
            if self.bare_marker(target) {
                self.mark_escape_why(target, &|_| {
                    "Reflect.construct: unknown constructor target".into()
                });
                self.reflect_construct(call, TOP, new_target);
                return;
            }
            self.new_target_unknown(call, new_target);
            self.construct(call, TOP);
            return;
        }
        let inst = match self.toks[new_target as usize] {
            TokKind::ClassObj(tm) => {
                let ctx = self.birth_ctx(call);
                self.site_tok(tm, ctx)
            }
            // A function/proxy or unknown new target need not select a
            // registered class layout. Its result and the body's this stay
            // unknown; a provisional instance of target would be false.
            _ => {
                self.new_target_unknown(call, new_target);
                TOP
            }
        };
        self.add_tok(c.ret, inst);
        match self.toks[target as usize] {
            TokKind::ClassObj(tm) => {
                self.run_ctor_chain(call, tm, inst);
                if self.chain_has_foreign_returning_ctor(tm) {
                    self.add_top(c.ret, |_| {
                        "Reflect.construct of a class whose constructor (another unit's) returns an object".into()
                    });
                }
            }
            TokKind::Func(f, _) => {
                self.call_tok(call, target, Some(inst));
                self.ctor_ret_objects(call, target, c.ret);
                if self.foreign_ctor.contains_key(&f) {
                    self.add_top(c.ret, |_| {
                        "Reflect.construct of another unit's constructor function that returns an object".into()
                    });
                }
            }
            _ => unreachable!("constructor target checked above"),
        }
    }
}
