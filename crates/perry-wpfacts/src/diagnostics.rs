//! Observational dump data. Never add constraints, nodes, tokens or proof inputs here.
use crate::gen::{Fx, Gen};
use crate::model::*;
use crate::pts::FxHashMap;
use perry_hir::Expr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct Trace {
    pub pred: BTreeMap<NodeId, (NodeId, String)>,
    pub seeds: BTreeMap<NodeId, String>,
    pub bound_origins: BTreeMap<u32, CallId>,
    pub source_calls: BTreeSet<CallId>,
    pub raw: BTreeMap<CallId, BTreeSet<Tok>>,
    pub builtins: BTreeMap<CallId, BTreeSet<String>>,
}

#[derive(Default)]
pub(crate) struct Capture {
    pub values: FxHashMap<usize, NodeId>,
    pub calls: FxHashMap<NodeId, CallId>,
    pub namespace_receivers: BTreeSet<NodeId>,
    sites: Vec<Site>,
}
struct Site {
    module: u32,
    function: FnIdx,
    start: u32,
    end: u32,
    kind: &'static str,
    key: String,
    origin: &'static str,
    receiver: Option<NodeId>,
    index: Option<NodeId>,
    callee: Option<NodeId>,
    result: NodeId,
    call: Option<CallId>,
}

impl Model {
    pub(crate) fn diagnostic_predecessor(&mut self, dst: NodeId, src: NodeId, reason: u32) {
        if let Some(d) = &mut self.diagnostic {
            d.pred.entry(dst).or_insert_with(|| {
                (
                    src,
                    self.reasons
                        .get(reason as usize)
                        .cloned()
                        .unwrap_or_default(),
                )
            });
        }
    }
    fn diagnostic_root(&self, start: NodeId) -> (NodeId, String) {
        let Some(d) = &self.diagnostic else {
            return (start, String::new());
        };
        let mut n = start;
        let mut seen = BTreeSet::new();
        while seen.insert(n) {
            if let Some(why) = d.seeds.get(&n) {
                return (n, why.clone());
            }
            let Some((src, why)) = d.pred.get(&n) else {
                return (n, String::new());
            };
            if !why.is_empty() {
                return (n, why.clone());
            }
            if *src == NO_NODE || !self.nodes[*src as usize].pts.contains_tok(TOP) {
                return (n, String::new());
            }
            n = *src;
        }
        (n, "cycle".into())
    }

    /// A summary instance keeps its template call identity. `CallSite::site`
    /// is an allocation context, and synthetic protocol calls may use zero:
    /// it must never be used to attribute their callees to source call zero.
    pub(crate) fn diagnostic_source_call(&self, call: CallId) -> Option<CallId> {
        let d = self.diagnostic.as_ref()?;
        if d.source_calls.contains(&call) {
            return Some(call);
        }
        let source = *self.sum.tmpl_of.get(&call)?;
        d.source_calls.contains(&source).then_some(source)
    }
    pub(crate) fn diagnostic_callee(&mut self, call: CallId, tok: Tok) {
        let Some(source) = self.diagnostic_source_call(call) else {
            return;
        };
        self.diagnostic
            .as_mut()
            .unwrap()
            .raw
            .entry(source)
            .or_default()
            .insert(tok);
    }
    pub(crate) fn diagnostic_builtin(&mut self, call: CallId, name: impl FnOnce(&Self) -> String) {
        let Some(source) = self.diagnostic_source_call(call) else {
            return;
        };
        let name = name(self);
        self.diagnostic
            .as_mut()
            .unwrap()
            .builtins
            .entry(source)
            .or_default()
            .insert(name);
    }
}

impl Gen<'_> {
    pub(crate) fn diagnostic_expr(&mut self, fx: &Fx, e: &Expr, result: NodeId) {
        let Some(d) = &mut self.diagnostic else {
            return;
        };
        let get = |e: &Expr| d.values.get(&(e as *const Expr as usize)).copied();
        let (start, end, kind, key, receiver, index, callee) = match e {
            Expr::PropertyGet {
                object,
                property,
                byte_offset,
                byte_end,
            } => (
                *byte_offset,
                *byte_end,
                "property",
                property.clone(),
                get(object),
                None,
                None,
            ),
            Expr::IndexGet {
                object,
                index,
                byte_offset,
                byte_end,
            } => (
                *byte_offset,
                *byte_end,
                "element",
                "[]".into(),
                get(object),
                get(index),
                None,
            ),
            Expr::Call {
                callee,
                byte_offset,
                byte_end,
                ..
            }
            | Expr::CallSpread {
                callee,
                byte_offset,
                byte_end,
                ..
            } => {
                let (recv, idx, key) = match callee.as_ref() {
                    Expr::PropertyGet {
                        object, property, ..
                    } => (get(object), None, property.clone()),
                    Expr::IndexGet { object, index, .. } => (get(object), get(index), "[]".into()),
                    _ => (None, None, "()".into()),
                };
                (
                    *byte_offset,
                    *byte_end,
                    "value-call",
                    key,
                    recv,
                    idx,
                    get(callee),
                )
            }
            Expr::New {
                class_name,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "new",
                class_name.clone(),
                None,
                None,
                None,
            ),
            Expr::NewDynamic {
                callee,
                byte_offset,
                byte_end,
                ..
            }
            | Expr::NewDynamicSpread {
                callee,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "new",
                "new".into(),
                None,
                None,
                get(callee),
            ),
            _ => {
                d.values.insert(e as *const Expr as usize, result);
                return;
            }
        };
        // Namespace lowering may not walk the syntactic receiver as an expression.
        let receiver = receiver.or_else(|| {
            self.read_sites
                .iter()
                .rev()
                .find(|s| s.module == fx.module && s.byte_offset == start && s.func == fx.f)
                .map(|s| s.recv)
        });
        let namespace = receiver.is_some_and(|n| d.namespace_receivers.contains(&n));
        let origin = if namespace {
            if end == 0 {
                "synthesized-namespace"
            } else {
                "namespace"
            }
        } else if start == 0 {
            "synthesized"
        } else if end == 0 {
            "transformed"
        } else {
            "source"
        };
        d.sites.push(Site {
            module: fx.module,
            function: fx.f,
            start,
            end,
            kind,
            key,
            origin,
            receiver,
            index,
            callee,
            result,
            call: d.calls.get(&result).copied(),
        });
        d.values.insert(e as *const Expr as usize, result);
    }
}

pub(crate) fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn nums<T: std::fmt::Display>(v: impl IntoIterator<Item = T>) -> String {
    format!(
        "[{}]",
        v.into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn opt(n: Option<u32>) -> String {
    n.map_or_else(|| "null".into(), |n| n.to_string())
}
fn strings(v: impl IntoIterator<Item = String>) -> String {
    format!(
        "[{}]",
        v.into_iter()
            .map(|x| quote(&x))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// A separate JSONL section: legacy S/T/F records and their digests stay untouched.
/// Node/token IDs are local to the enclosing emitting analysis unit.
pub(crate) fn collect(g: &Gen) -> Vec<String> {
    let Some(d) = &g.diagnostic else {
        return Vec::new();
    };
    let m = &g.m;
    let mut out = vec![format!(
        "{{\"record\":\"analysis\",\"aborted\":{},\"work\":{},\"maxWork\":{}}}",
        m.aborted, m.work, m.max_work
    )];
    let mut nodes = BTreeSet::new();
    let locations: BTreeMap<CallId, &Site> = d
        .sites
        .iter()
        .filter_map(|s| s.call.map(|c| (c, s)))
        .collect();
    for s in &d.sites {
        nodes.extend(
            [s.receiver, s.index, s.callee, Some(s.result)]
                .into_iter()
                .flatten(),
        );
        let call = s.call.map(|c| &m.calls[c as usize]);
        if let Some(c) = call {
            nodes.extend(c.args.iter().chain(&c.spread).copied());
        }
        let raw = s.call.and_then(|c| m.diagnostic.as_ref()?.raw.get(&c));
        let bi = s.call.and_then(|c| m.diagnostic.as_ref()?.builtins.get(&c));
        out.push(format!("{{\"record\":\"site\",\"file\":{},\"function\":{},\"start\":{},\"end\":{},\"kind\":{},\"key\":{},\"origin\":{},\"receiver\":{},\"index\":{},\"callee\":{},\"result\":{},\"call\":{},\"raw\":{},\"builtins\":{},\"args\":{},\"spread\":{}}}",
            quote(&g.modules[s.module as usize].0),s.function,s.start,s.end,quote(s.kind),quote(&s.key),quote(s.origin),
            opt(s.receiver),opt(s.index),opt(s.callee),s.result,opt(s.call),
            nums(raw.into_iter().flatten()),strings(bi.into_iter().flatten().cloned()),
            nums(call.into_iter().flat_map(|c|c.args.iter())),nums(call.into_iter().flat_map(|c|c.spread.iter()))));
    }
    for (i, f) in m.funcs.iter().enumerate() {
        nodes.extend(f.params.iter().chain([&f.this, &f.ret]));
        let module = g.modules.get(f.module as usize);
        let src = module.and_then(|(_, m)| m.closure_source_text.get(&f.func_id));
        let span = src.and_then(|s| s.source_span).unwrap_or((0, 0));
        let declaration_key = g
            .stand_in_fc
            .get(&f.token)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| crate::fn_contract_name(g, i as FnIdx).0);
        out.push(format!("{{\"record\":\"function\",\"id\":{i},\"hirId\":{},\"declarationKey\":{},\"file\":{},\"name\":{},\"start\":{},\"end\":{},\"params\":{},\"this\":{},\"result\":{},\"rest\":{},\"parent\":{},\"escape\":{}}}",
            f.func_id,quote(&declaration_key),quote(module.map_or("",|(p,_)|*p)),quote(&f.name),span.0,span.1,nums(&f.params),f.this,f.ret,opt(f.rest),opt((f.parent!=NO_FN).then_some(f.parent)),
            quote(m.escaped.get(&f.token).map_or("",|r|&m.reasons[*r as usize]))));
    }
    for (i, t) in m.tmpls.iter().enumerate() {
        let (module, classid) = match &t.kind {
            TmplKind::Class {
                module, class_id, ..
            } => (Some(*module), Some(*class_id)),
            _ => (None, None),
        };
        let source = module
            .and_then(|mi| g.modules.get(mi as usize))
            .and_then(|(_, m)| m.class_source_text.get(&classid?));
        let span = source.map(|s| s.source_range()).unwrap_or(0..0);
        let mut ancestors = Vec::new();
        let mut next = t.parent;
        while let Some(a) = next {
            if ancestors.contains(&a) {
                break;
            }
            ancestors.push(a);
            next = m.tmpls[a as usize].parent;
        }
        out.push(format!("{{\"record\":\"layout\",\"id\":{i},\"name\":{},\"kind\":{},\"file\":{},\"start\":{},\"end\":{},\"parent\":{},\"parentLinks\":{},\"ancestors\":{},\"nativeParent\":{}}}",
            quote(&t.display),quote(&format!("{:?}",t.kind)),quote(module.map_or("",|mi|g.modules[mi as usize].0)),
            if source.is_some(){span.start+1}else{0},if source.is_some(){span.end+1}else{0},opt(t.parent),nums(&t.parent_links),nums(ancestors),quote(&format!("{:?}",t.native_parent))));
    }
    // Fields already reached by the solver, including independent any-key TOP.
    // Reading these maps must never call field()/load_from() and create flow.
    let mut fields: Vec<_> = m
        .fields
        .iter()
        .map(|(&(t, a), &n)| (t, m.atoms[a as usize].clone(), n))
        .collect();
    fields.extend(m.field_any.iter().map(|(&t, &n)| (t, "*".into(), n)));
    fields.sort();
    for (t, key, n) in fields {
        if m.nodes[n as usize].pts.contains_tok(TOP) {
            nodes.insert(n);
            out.push(format!(
                "{{\"record\":\"top-field\",\"token\":{t},\"key\":{},\"node\":{n}}}",
                quote(&key)
            ));
        }
    }
    for (i, t) in m.toks.iter().enumerate() {
        let (kind, layout, function, context) = match t {
            TokKind::Top => ("top", None, None, None),
            TokKind::Obj(l, c) => ("object", Some(*l), None, Some(*c)),
            TokKind::Func(f, c) => ("function", None, Some(*f), Some(*c)),
            TokKind::Bound(_) => ("bound", None, None, None),
            TokKind::ClassObj(l) => ("class", Some(*l), None, None),
            TokKind::Proto(l) => ("prototype", Some(*l), None, None),
            TokKind::Builtin(_) | TokKind::BuiltinFn => ("builtin", None, None, None),
            TokKind::ModNs(_) => ("namespace", None, None, None),
            TokKind::ObjectProto | TokKind::NativeProto(_) => {
                ("native-prototype", None, None, None)
            }
        };
        let bound = if let TokKind::Bound(b) = t {
            let (target, this, args) = &m.bound[*b as usize];
            nodes.extend([target, this]);
            nodes.extend(args);
            let origin = m
                .diagnostic
                .as_ref()
                .and_then(|d| d.bound_origins.get(b))
                .and_then(|c| locations.get(c))
                .map_or("null".into(), |s| {
                    format!(
                        "{{\"file\":{},\"start\":{},\"end\":{}}}",
                        quote(g.modules[s.module as usize].0),
                        s.start,
                        s.end
                    )
                });
            format!(
                "{{\"target\":{target},\"this\":{this},\"args\":{},\"origin\":{origin}}}",
                nums(args)
            )
        } else {
            "null".into()
        };
        out.push(format!("{{\"record\":\"token\",\"id\":{i},\"kind\":{},\"name\":{},\"layout\":{},\"function\":{},\"context\":{},\"bound\":{},\"escape\":{},\"marker\":{},\"closedMarker\":{}}}",
            quote(kind),quote(&m.tok_name(i as Tok)),opt(layout),opt(function),opt(context),bound,
            quote(m.escaped.get(&(i as Tok)).map_or("",|r|&m.reasons[*r as usize])), m.is_marker(i as Tok), m.closed_marker(i as Tok)));
    }
    let roots: Vec<_> = nodes.iter().map(|&n| m.diagnostic_root(n).0).collect();
    nodes.extend(roots);
    for n in nodes {
        let v = &m.nodes[n as usize];
        let toks = v.pts.to_sorted();
        let top = toks.contains(&TOP);
        let marker_top = toks.iter().any(|&t| m.bare_marker(t));
        let mut mask = v.prim as u32;
        for &t in &toks {
            if m.is_marker(t) {
                continue;
            }
            mask |= match m.toks[t as usize] {
                TokKind::Top => 0,
                TokKind::Func(..)
                | TokKind::Bound(_)
                | TokKind::ClassObj(_)
                | TokKind::BuiltinFn => 256,
                TokKind::Builtin(ns)
                    if !matches!(
                        ns,
                        BuiltinNs::Json
                            | BuiltinNs::Math
                            | BuiltinNs::Console
                            | BuiltinNs::GlobalThis
                            | BuiltinNs::Process
                            | BuiltinNs::Other
                    ) =>
                {
                    256
                }
                _ => 128,
            };
        }
        let (root, origin) = m.diagnostic_root(n);
        out.push(format!("{{\"record\":\"node\",\"id\":{n},\"primitiveMask\":{},\"kindMask\":{mask},\"top\":{top},\"markerTop\":{marker_top},\"tokens\":{},\"label\":{},\"parameter\":{},\"observation\":{},\"topRoot\":{},\"topOrigin\":{}}}",
            v.prim,nums(toks),quote(m.labels.get(n as usize).copied().unwrap_or("")),v.param,v.obs,opt(top.then_some(root)),quote(&if top&&m.explain{m.top_origin(n)}else{origin})));
    }
    out
}
