//! Constraint generation, part 4b: the helpers `expr_rest` calls (iterator
//! results, property definition, descriptors, JSON values, error objects,
//! runtime glue, CommonJS factory slots).

use crate::gen::{Fx, Gen};
use crate::model::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    pub(crate) fn iter_res_node(&mut self, fx: &Fx) -> NodeId {
        if let Some(&n) = self.mods[fx.module as usize].iterres.get(&fx.f) {
            return n;
        }
        let n = self.tmp("iterres");
        self.mods[fx.module as usize].iterres.insert(fx.f, n);
        n
    }

    /// `Object.defineProperty(o, key, desc)`: E5 on the target, the
    /// descriptor's value is a post-construction write, and its accessors
    /// become accessors of the target (called with `this` = the target).
    /// `accessor`: the descriptor is an accessor descriptor (FVA 4.3a): with
    /// a constant key the definition is an accessor key on every layout that
    /// does not hold the key as data (`Cons::DefineAccessor`), not E5 HARD.
    pub(crate) fn define_property(
        &mut self,
        fx: &Fx,
        o: NodeId,
        key: Option<String>,
        desc: NodeId,
        r: u32,
        accessor: bool,
    ) {
        match &key {
            Some(k) if accessor => {
                let ka = self.m.atom(k);
                self.m.cons(o, Cons::DefineAccessor { key: ka, reason: r });
            }
            _ => self.m.cons(
                o,
                Cons::Hard {
                    kind: Hard::E5DefineProperty,
                    reason: r,
                },
            ),
        }
        let va = self.m.atom("value");
        let v = self.load(desc, Key::Named(va));
        let k = match &key {
            Some(s) => Key::Named(self.m.atom(s)),
            None => Key::Any,
        };
        self.store(fx, o, k, v, "defineProperty".into(), None, false);
        let ka = match &key {
            Some(s) => self.m.atom(s),
            None => self.m.atom(crate::STAR_KEY),
        };
        if key.is_some() {
            self.m.accessor_atoms.insert(ka);
        }
        for (name, setter) in [("get", false), ("set", true)] {
            let a = self.m.atom(name);
            let f = self.load(desc, Key::Named(a));
            self.m.cons(
                f,
                Cons::Accessor {
                    holder: o,
                    key: ka,
                    setter,
                },
            );
        }
    }

    pub(crate) fn descriptor_of(&mut self, mi: u32, o: NodeId) -> NodeId {
        let r = self
            .m
            .reason("getOwnPropertyDescriptor (accessors handed out)");
        self.m.cons(o, Cons::Implicit { reason: r });
        let vals = self.load(o, Key::Any);
        let (t, n) = self.bobj(mi, "descriptor");
        let fa = self.m.field_any(t);
        self.m.edge(vals, fa);
        // get/set functions of the object: handed to user code without a
        // receiver — ⊤-called.
        self.m.add_tok(fa, TOP);
        self.m.add_prim(fa, prim::ALL);
        n
    }

    pub(crate) fn json_value(&mut self, mi: u32) -> NodeId {
        let (t, n) = self.bobj(mi, "json");
        let fa = self.m.field_any(t);
        self.m.add_prim(fa, prim::ALL);
        self.m.add_tok(fa, t);
        let arr = self.new_arr_template(mi, "json");
        let at = self.m.tok(TokKind::Obj(arr, NO_CTX));
        let afa = self.m.field_any(at);
        self.m.add_prim(afa, prim::ALL);
        self.m.add_tok(afa, t);
        self.m.add_tok(afa, at);
        self.m.add_tok(fa, at);
        let out = self.tmp("json");
        self.m.edge(n, out);
        self.m.add_tok(out, at);
        self.m.add_prim(out, prim::ALL);
        out
    }

    pub(crate) fn error_obj(&mut self, mi: u32, cause: Option<NodeId>) -> NodeId {
        let (t, n) = self.bobj(mi, "Error");
        for k in ["message", "stack", "name"] {
            let a = self.m.atom(k);
            let f = self.m.field(t, a);
            self.m.add_prim(f, prim::STR);
        }
        if let Some(c) = cause {
            let a = self.m.atom("cause");
            let f = self.m.field(t, a);
            self.m.edge(c, f);
        }
        n
    }

    /// `__perry_runtime` glue calls emitted by HIR lowering (iteration,
    /// destructuring, CJS module records).
    pub(crate) fn runtime_glue(
        &mut self,
        fx: &Fx,
        method: &str,
        object: Option<NodeId>,
        args: Vec<NodeId>,
        source_args: &[Expr],
    ) -> NodeId {
        let arg = |i: usize| args.get(i).copied();
        match method {
            "arrayRecordNeedsIterator" | "arrayRecordForBound" | "hasPathModule" => {
                self.prim_node(prim::BOOL)
            }
            // What codegen lowers them to: the cursor itself, the payload's
            // length, and `mode ? 0 : update`.
            "arrayRecordIndex" => match arg(0) {
                Some(a) => a,
                None => self.prim_node(prim::NUM),
            },
            "arrayRecordLength" => self.prim_node(prim::NUM),
            "arrayRecordForUpdate" => {
                let out = self.tmp("glue-update");
                self.m.add_prim(out, prim::NUM);
                if let Some(a) = arg(1) {
                    self.m.edge(a, out);
                }
                out
            }
            "requireResolveNodeModules" => self.prim_node(prim::STR | prim::UNDEF),
            // The value checked, nothing else: a second argument is the
            // destructuring source offset (`--debug-symbols`) for the error
            // location, never the result (`js_require_object_coercible`).
            "requireObjectCoercible" => {
                let out = self.tmp("glue-pass");
                if let Some(a) = arg(0) {
                    self.m.edge(a, out);
                }
                out
            }
            "arrayRecordPayload" | "arrayRecordForValue" => {
                let out = self.tmp("glue-pass");
                for a in &args {
                    self.m.edge(*a, out);
                }
                // The payload of a for-of record is iterated by the loop.
                if method == "arrayRecordPayload" {
                    if let Some(a) = arg(0) {
                        let it = self.iterate(a);
                        let io = self.iter_out_node(fx);
                        self.m.edge(it, io);
                    }
                }
                out
            }
            "iteratorNextMethod" => {
                let Some(it) = arg(0) else {
                    return self.m.top_val("iteratorNextMethod without argument");
                };
                let out = self.tmp("nextmethod");
                self.m.cons(it, Cons::IterNext { dst: out });
                out
            }
            "iteratorStep" => {
                let (Some(it), Some(next)) = (arg(0), arg(1)) else {
                    return self.prim_node(prim::BOOL);
                };
                let res =
                    self.call_value(next, ThisArg::Node(it), vec![self.m.top_tok_node], vec![]);
                let va = self.m.atom("value");
                let v = self.load(res, Key::Named(va));
                let io = self.iter_out_node(fx);
                self.m.edge(v, io);
                let all = self.iterate(it);
                self.m.edge(all, io);
                self.prim_node(prim::BOOL)
            }
            "iteratorStepOutput" => self.iter_out_node(fx),
            "iteratorCloseOnThrow" | "iteratorCloseIfNotDone" | "arrayRecordClose" => {
                if let Some(it) = arg(0) {
                    // `iterator.return()` runs.
                    let ra = self.m.atom("return");
                    let _ = self.mcall(it, Key::Named(ra), vec![], vec![]);
                }
                let out = self.tmp("close");
                self.m.edge(self.thrown, out);
                for a in args.iter().skip(1) {
                    self.m.edge(*a, out);
                }
                out
            }
            "registerPathModule" | "registerPathModulePartial" => {
                if let Some(value) = arg(1) {
                    let registry = match source_args.first() {
                        Some(Expr::String(path)) if std::path::Path::new(path).is_absolute() => {
                            self.cjs_path_node(path)
                        }
                        _ => self.cjs_unknown_path,
                    };
                    // A CommonJS record publishes its live `exports`; a raw
                    // publication is the value itself. The registry union
                    // keeps the record for `require.cache`.
                    self.m.cons(value, Cons::CjsPublish { dst: registry });
                    self.m.edge(value, self.cjs_registry);
                }
                self.prim_node(prim::UNDEF)
            }
            "newCjsModuleRecord" => {
                // `CJS_MODULE_RECORD`: a fresh record per call whose fields
                // are the arguments (exports, filename, dir, children,
                // parent, paths) plus the marker, `loaded` and `require`.
                let (_, record) = self.bobj(fx.module, "CjsRecord");
                let fields: [(&str, Option<usize>, u16); 10] = [
                    ("exports", Some(0), prim::UNDEF),
                    ("__perry_cjs_record", None, prim::BOOL),
                    ("id", Some(1), prim::UNDEF),
                    ("path", Some(2), prim::UNDEF),
                    ("filename", Some(1), prim::UNDEF),
                    ("loaded", None, prim::BOOL),
                    ("children", Some(3), prim::UNDEF),
                    ("parent", Some(4), prim::UNDEF),
                    ("paths", Some(5), prim::UNDEF),
                    ("require", None, prim::UNDEF),
                ];
                for (name, from, fallback) in fields {
                    let value = match from.and_then(arg) {
                        Some(v) => v,
                        None => self.prim_node(fallback),
                    };
                    let key = self.m.atom(name);
                    self.store(
                        fx,
                        record,
                        Key::Named(key),
                        value,
                        "cjs record".into(),
                        None,
                        false,
                    );
                }
                if let Some(factory) = arg(6) {
                    self.cjs_factory_slot(fx, record, factory);
                }
                record
            }
            "setCjsFactory" => {
                if let (Some(record), Some(factory)) = (arg(0), arg(1)) {
                    self.cjs_factory_slot(fx, record, factory);
                }
                self.prim_node(prim::UNDEF)
            }
            "linkPathModuleParent" => {
                // The helper links an existing record through require.cache;
                // it does not publish a new path-module value.
                // Keep the record in the dynamic loader summary.
                if let Some(record) = arg(0) {
                    self.m.edge(record, self.cjs_registry);
                }
                self.prim_node(prim::UNDEF)
            }
            "requirePathModule" => {
                // A path an instance knows is as good as a literal.
                let known = match source_args.first().map(|x| self.const_eval(fx.module, x)) {
                    Some(Some(crate::gen_const::Const::Str(s))) => Some(Expr::String(s)),
                    _ => None,
                };
                let registry = match known.as_ref().or(source_args.first()) {
                    Some(Expr::String(path)) if std::path::Path::new(path).is_absolute() => {
                        let named = self.cjs_path_node(path);
                        let r = self.tmp("cjs-path-read");
                        self.m.edge(named, r);
                        self.m.edge(self.cjs_unknown_path, r);
                        // The runtime also accepts directory package-main and
                        // index paths. Keep those on the summary until their
                        // resolver is modeled here as well.
                        if std::path::Path::new(path).is_dir() {
                            self.m.edge(self.cjs_registry, r);
                        }
                        r
                    }
                    _ => self.cjs_registry,
                };
                let out = self.tmp("require");
                // Raw publications remain supported; CJS records unwrap their
                // live exports, including replacements during a cycle.
                self.m.edge(registry, out);
                let ea = self.m.atom("exports");
                let ex = self.load(registry, Key::Named(ea));
                self.m.edge(ex, out);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            "getCjsMainModule" => self.cjs_main_value(fx.module),
            "setCjsMainModule" => {
                let main = self.cjs_main_value(fx.module);
                if let Some(record) = arg(0) {
                    self.m.edge(record, main);
                    self.to_sink(record, "CJS main record published to process.mainModule");
                }
                self.prim_node(prim::UNDEF)
            }
            _ => {
                let why = format!("argument of runtime glue {method}");
                for n in object.into_iter().chain(args) {
                    self.to_sink(n, &why);
                }
                self.m.top_val(&format!("result of runtime glue {method}"))
            }
        }
    }

    /// A CommonJS record's private factory slot. The slot belongs to the
    /// record: whatever reaches the record's contents (an escape) reaches the
    /// factory. The runtime calls it, with no receiver and no arguments, when
    /// a loaded module is required again after its cache entry was deleted.
    pub(crate) fn cjs_factory_slot(&mut self, fx: &Fx, record: NodeId, factory: NodeId) {
        let key = self.m.atom("__perry_cjs_factory");
        self.store(
            fx,
            record,
            Key::Named(key),
            factory,
            "cjs factory slot".into(),
            None,
            false,
        );
        let _ = self.call_value(factory, ThisArg::Undef, vec![], vec![]);
    }

    pub(crate) fn iter_out_node(&mut self, fx: &Fx) -> NodeId {
        if let Some(&n) = self.mods[fx.module as usize].iterout.get(&fx.f) {
            return n;
        }
        let n = self.tmp("iterout");
        self.mods[fx.module as usize].iterout.insert(fx.f, n);
        n
    }
}
