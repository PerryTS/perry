use super::*;

/// A published class's static members along its chain inside the unit
/// (first one wins, as a lookup does): the key and the static method's
/// name when the member holds exactly one function; and whether the class
/// object may hold other keys (a computed static store, a native parent).
pub(super) fn class_statics(
    g: &gen::Gen,
    tm: model::TmplId,
) -> (Vec<(String, Option<model::FnIdx>)>, bool) {
    let pa = g.m.atoms.iter().position(|x| x == "prototype");
    let mut out: Vec<(String, Option<model::FnIdx>)> = Vec::new();
    let mut open = false;
    let mut cur = Some(tm);
    while let Some(c) = cur {
        let Some(&cobj) = g.m.tok_index.get(&model::TokKind::ClassObj(c)) else {
            break;
        };
        if let Some(&an) = g.m.field_any.get(&cobj) {
            let node = &g.m.nodes[an as usize];
            open |= node.prim != 0 || !node.pts.is_empty();
        }
        open |= g.m.tmpls[c as usize].native_parent.is_some();
        let mut own: Vec<(String, Option<model::FnIdx>)> =
            g.m.field_keys
                .get(&cobj)
                .into_iter()
                .flatten()
                .filter(|&&k| Some(k as usize) != pa && !g.m.is_private_atom(k))
                .filter_map(|&k| {
                    let n = *g.m.fields.get(&(cobj, k))?;
                    let ts = g.m.nodes[n as usize].pts.to_sorted();
                    let f = ts.first().map(|&t| g.m.tok_fn(t)).filter(|&f| {
                        ts.len() == 1 && f != model::NO_FN && g.m.nodes[n as usize].prim == 0
                    });
                    Some((g.m.atoms[k as usize].clone(), f))
                })
                .filter(|(k, _)| !out.iter().any(|(o, _)| o == k))
                .collect();
        own.sort();
        out.extend(own);
        cur = g.m.tmpls[c as usize].parent;
    }
    (out, open)
}

/// The templates whose objects the defining unit stored into under a
/// computed key: a reader may find any key on them.
pub(super) fn open_templates(g: &gen::Gen) -> pts::FxHashSet<model::TmplId> {
    g.m.field_any
        .iter()
        .filter(|&(_, &n)| {
            let node = &g.m.nodes[n as usize];
            node.prim != 0 || !node.pts.is_empty()
        })
        .filter_map(|(&h, _)| g.m.tmpl_of(h))
        .collect()
}

/// The fields of literal template `tm` (its tokens among `toks`) that hold
/// only contract functions of this unit (`Gen::note_field_fns`), each with
/// their function contracts.
pub(super) fn literal_field_fns(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    toks: &[model::Tok],
    tm: model::TmplId,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> Vec<(String, Vec<units::FnContract>)> {
    let mine: Vec<model::Tok> = toks
        .iter()
        .copied()
        .filter(|&t| g.m.tmpl_of(t) == Some(tm))
        .collect();
    let mut keys: Vec<model::Atom> = mine
        .iter()
        .flat_map(|t| g.m.field_keys.get(t).into_iter().flatten().copied())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let mut out = Vec::new();
    'key: for a in keys {
        // A class private field is no key of the layout to a reader.
        if g.m.is_private_atom(a) {
            continue;
        }
        let mut fs: Vec<(model::FnIdx, model::Tok)> = Vec::new();
        for &t in &mine {
            let Some(&n) = g.m.fields.get(&(t, a)) else {
                continue;
            };
            if !g.field_fns.contains_key(&(t, a)) {
                continue 'key;
            }
            let node = &g.m.nodes[n as usize];
            if node.prim != 0 {
                continue 'key;
            }
            for ft in node.pts.to_sorted() {
                match g.m.toks[ft as usize] {
                    model::TokKind::Func(f, _)
                        if g.m.contract_fns.contains(&f) || g.open_fn(ft) =>
                    {
                        if !fs.iter().any(|x| x.0 == f) {
                            fs.push((f, ft));
                        }
                    }
                    _ => continue 'key,
                }
            }
        }
        if fs.is_empty() {
            continue;
        }
        fs.sort_unstable();
        // Another unit's open function forwards its own contract (§18.9).
        let fcs = fs
            .into_iter()
            .map(|(f, ft)| match g.stand_in_fc.get(&ft) {
                Some(&c) if g.m.funcs[f as usize].func_id == u32::MAX => forwarded_fn(g, c),
                _ => fn_contract_of(g, templates, f, layouts_out, open_tm),
            })
            .collect();
        out.push((g.m.atoms[a as usize].clone(), fcs));
    }
    out
}

/// The value contracts of literal template `tm`'s fields that forward
/// other units' stand-ins (DECISIONS 88, `Gen::nested_fields`): per key the
/// join over every instance that left.
pub(super) fn field_value_contracts(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    tm: model::TmplId,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> Vec<(String, units::ValueContract)> {
    let mut keys: Vec<(model::Atom, Vec<model::NodeId>)> = g
        .field_values
        .iter()
        .filter(|&(&(t, _), _)| t == tm)
        .map(|(&(_, a), v)| (a, v.clone()))
        .collect();
    keys.sort_unstable();
    let mut out = Vec::new();
    for (a, ns) in keys {
        let mut vc = units::ValueContract::default();
        for n in ns {
            vc.join(&value_contract_of(
                g,
                templates,
                n,
                &[],
                layouts_out,
                open_tm,
            ));
        }
        out.push((g.m.atoms[a as usize].clone(), vc));
    }
    out
}

/// DECISIONS 126: every value the closed parameter node `p` holds (its
/// callers' described arguments) holds only stand-in typed arrays under key
/// `a`. Before any caller published (round 1) it holds nothing: bottom, as
/// decision 13's closed parameters start (DECISIONS 123); the callers'
/// arguments make it grow and the body is solved again.
pub(super) fn described_ta_key(g: &gen::Gen, p: model::NodeId, a: model::Atom) -> bool {
    for t in g.m.nodes[p as usize].pts.to_sorted() {
        if g.m.param_marker.contains_key(&t) {
            continue;
        }
        let Some(&n) = g.m.fields.get(&(t, a)) else {
            return false;
        };
        let ts = g.m.nodes[n as usize].pts.to_sorted();
        if ts.is_empty()
            || !ts
                .iter()
                .all(|x| g.stand_in_tas.iter().any(|&(_, s)| s == *x))
        {
            return false;
        }
    }
    true
}

/// The typed-array fields of template `tm` (DECISIONS 126,
/// `Gen::nested_fields`): key, element kind, primitives beside.
pub(super) fn field_ta_contracts(
    g: &gen::Gen,
    tm: model::TmplId,
) -> Vec<(String, Option<u8>, u16)> {
    let mut out: Vec<(String, Option<u8>, u16)> = g
        .field_tas
        .iter()
        .filter(|&(&(t, _), _)| t == tm)
        .map(|(&(_, a), &(e, p))| (g.m.atoms[a as usize].clone(), e, p))
        .collect();
    out.sort();
    out
}

/// The nested contracts of literal template `tm`'s fields (design §18.7,
/// `Gen::nested_fields`): per described key the layouts it holds, each
/// published in `layouts_out` with its own nested contracts.
pub(super) fn nested_layouts(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    tm: model::TmplId,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> Vec<(String, Vec<String>, bool)> {
    let mut out = Vec::new();
    let mut keys: Vec<(model::Atom, Vec<model::TmplId>)> = g
        .field_layouts
        .iter()
        .filter(|&(&(t, _), _)| t == tm)
        .map(|(&(_, a), v)| (a, v.clone()))
        .collect();
    keys.sort_unstable();
    for (a, itms) in keys {
        let mut ds = Vec::new();
        for itm in itms {
            let display = g.m.tmpls[itm as usize].display.clone();
            // Another unit's layout: its owner publishes it.
            if g.forwarded_layout(itm) {
                ds.push(display);
                continue;
            }
            if !layouts_out.iter().any(|c| c.display == display) {
                let facts = templates.iter().find(|x| x.id == itm).cloned();
                // `Gen::nested_ok` admits only literal layouts of this unit.
                let (name, class_id) = match g.m.tmpls[itm as usize].kind.clone() {
                    model::TmplKind::Class { name, class_id, .. } => (name, class_id),
                    _ => (display.clone(), 0),
                };
                let facts = facts.expect("a nested layout has template facts");
                layouts_out.push(units::ClassContract {
                    display: display.clone(),
                    name,
                    class_id,
                    literal_shape: true,
                    facts,
                    methods: Vec::new(),
                    getters: Vec::new(),
                    own_getters: own_getters(&g, itm),
                    native_parent: None,
                    parent: foreign_parent(&g, itm),
                    field_fns: Vec::new(),
                    field_layouts: Vec::new(),
                    field_values: Vec::new(),
                    field_tas: Vec::new(),
                    open_keys: open_tm.contains(&itm),
                    statics: Vec::new(),
                    statics_open: false,
                    static_fns: Vec::new(),
                    static_values: Vec::new(),
                    closed: None,
                    forwards: false,
                    method_fns: Vec::new(),
                    ctor_fn: None,
                    // The shape's own birth keys: a reader's stand-in judges
                    // birth fields and the CommonJS record marker on them
                    // (DECISIONS 101).
                    birth_keys: atoms_of(g, &g.m.tmpls[itm as usize].birth_keys),
                    definite: atoms_of(g, &g.m.tmpls[itm as usize].definite),
                    reserved: atoms_of(g, &g.m.tmpls[itm as usize].reserved),
                    ctor_leaks: true,
                    ctor_returns: g.chain_returns(itm),
                    binding_prim: 0,
                    fn_inst: false,
                });
                let nl = nested_layouts(g, templates, itm, layouts_out, open_tm);
                let fv = field_value_contracts(g, templates, itm, layouts_out, open_tm);
                if let Some(c) = layouts_out.iter_mut().find(|c| c.display == display) {
                    c.field_layouts = nl;
                    c.field_values = fv;
                    c.field_tas = field_ta_contracts(g, itm);
                }
            }
            ds.push(display);
        }
        out.push((
            g.m.atoms[a as usize].clone(),
            ds,
            g.field_prims.contains(&(tm, a)),
        ));
    }
    out.sort();
    out
}

/// The function contract of contract function `f` after the solve (design
/// §11.2): per parameter what the callee does with its argument (the
/// markers' uses), the layouts it returns (added to `layouts_out`), the
/// result transfers.
/// A contract function's name and, when it is closed, its closed key. A
/// closed local (decision 23) is named by its function id too: locals of one
/// name (three `loader`s in moduleNameResolver.ts) are different functions,
/// with their own census entry and stand-in.
pub(super) fn fn_contract_name(g: &gen::Gen, f: model::FnIdx) -> (String, Option<String>) {
    let fi = &g.m.funcs[f as usize];
    let fid = fi.func_id;
    let closed = g.view.and_then(|v| {
        v.closed
            .iter()
            .find(|&(_, &id)| id == fid)
            .map(|(nm, _)| nm.clone())
    });
    let name = match &closed {
        Some(k) if k.contains('#') && !fi.name.ends_with(&format!("#{fid}")) => {
            format!("{}#{fid}", fi.name)
        }
        _ => fi.name.clone(),
    };
    (name, closed)
}

/// A class up whose chain an `extends` value holds no class yet (an empty
/// parent cell in an early round): bottom, no instance of it exists yet,
/// so no contract describes it this round (DECISIONS 105). Published as
/// it stands, a reader would judge its instances by a chain without the
/// parent and break it for good; its unit publishes it once the value
/// arrives, and the layouts its readers read re-solve them (DECISIONS 103).
pub(super) fn chain_unsettled(g: &gen::Gen, tm: model::TmplId) -> bool {
    let mut cur = Some(tm);
    while let Some(c) = cur {
        let t = &g.m.tmpls[c as usize];
        if g.m.foreign.contains_key(&c) {
            return false;
        }
        if let (None, Some((pv, _))) = (t.parent, t.parent_value) {
            let n = &g.m.nodes[pv as usize];
            // `extends` of a primitive other than null throws when the
            // class is defined: no instance with that parent exists either.
            if n.pts.is_empty() && n.prim & model::prim::NULL == 0 {
                return true;
            }
        }
        cur = t.parent;
    }
    false
}

/// The getters on `tm`'s prototype chain inside this unit, as `methods`
/// walks it (`ClassContract::getters`): key and function name.
pub(super) fn proto_getters(g: &gen::Gen, tm: model::TmplId) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut cur = Some(tm);
    while let Some(c) = cur {
        if g.m.foreign.contains_key(&c) {
            break;
        }
        if let Some(&proto) = g.m.tok_index.get(&model::TokKind::Proto(c)) {
            for &a in g.m.accessor_keys.get(&proto).into_iter().flatten() {
                let Some((gs, _)) = g.m.accessors.get(&(proto, a)) else {
                    continue;
                };
                for &t in gs {
                    let f = g.m.tok_fn(t);
                    if f == model::NO_FN {
                        continue;
                    }
                    let e = (
                        g.m.atoms[a as usize].clone(),
                        g.m.funcs[f as usize].name.clone(),
                    );
                    if !out.contains(&e) {
                        out.push(e);
                    }
                }
            }
        }
        cur = g.m.tmpls[c as usize].parent;
    }
    out.sort();
    out
}

/// The own accessors of literal layout `tm` (`ClassContract::own_getters`):
/// every accessor installed on an object of `tm` (its birth defines them,
/// `js_object_define_accessor`), one entry per key: its contract getter
/// (`Model::contract_getter`, published under its contract name), a
/// setter only, or a getter no contract describes. The key hides the
/// prototype chain for the reader, as it does in the owner.
pub(super) fn own_getters(g: &gen::Gen, tm: model::TmplId) -> Vec<(String, units::OwnAccessor)> {
    use units::OwnAccessor;
    let mut out: Vec<(String, OwnAccessor)> = Vec::new();
    for (&(h, a), (gs, ss)) in g.m.accessors.iter() {
        if !matches!(g.m.toks[h as usize], model::TokKind::Obj(..)) || g.m.tmpl_of(h) != Some(tm) {
            continue;
        }
        if gs.is_empty() && ss.is_empty() {
            continue;
        }
        let acc = match g.m.contract_getter(h, a) {
            Some(f) => OwnAccessor::Getter(fn_contract_name(g, f).0),
            None if gs.is_empty() => OwnAccessor::SetterOnly,
            None => OwnAccessor::Unknown,
        };
        let key = &g.m.atoms[a as usize];
        match out.iter_mut().find(|(k, _)| k == key) {
            // Two objects of the layout that disagree: no contract.
            Some((_, x)) if *x != acc => *x = OwnAccessor::Unknown,
            Some(_) => {}
            None => out.push((key.clone(), acc)),
        }
    }
    out.sort();
    out
}

/// The first class up `tm`'s chain that is another unit's (a stand-in),
/// by display (DECISIONS 104): the chain below it is this unit's own.
pub(super) fn foreign_parent(g: &gen::Gen, tm: model::TmplId) -> Option<String> {
    let mut cur = g.m.tmpls[tm as usize].parent;
    while let Some(c) = cur {
        if g.m.foreign.contains_key(&c) {
            return Some(g.m.tmpls[c as usize].display.clone());
        }
        cur = g.m.tmpls[c as usize].parent;
    }
    None
}

pub(super) fn atoms_of(g: &gen::Gen, ks: &[model::Atom]) -> Vec<String> {
    ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect()
}

#[path = "contracts/fn_contracts.rs"]
mod fn_contracts;
pub(crate) use fn_contracts::*;
/// A reach marker reaches itself under every key (§18.4): what it stands
/// for in the result is its reach through the keys the body read on it,
/// repeatedly; a computed-key read (its all-fields node feeds a reader)
/// is any key (DECISIONS 138).
pub(super) fn keys_read(g: &gen::Gen, t: model::Tok) -> Option<Vec<String>> {
    if g.m
        .all_fields
        .get(&t)
        .is_some_and(|&n| !g.m.nodes[n as usize].succ.is_empty())
    {
        return None;
    }
    let mut v: Vec<String> = g.m.field_keys.get(&t).map_or_else(Vec::new, |ks| {
        ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect()
    });
    v.sort();
    v.dedup();
    Some(v)
}

/// DECISIONS 158: a parameter's path marker stands for the values at
/// its path, each step with the keys its folds repeat there.
pub(super) fn path_loops(g: &gen::Gen, t: model::Tok) -> Option<Vec<String>> {
    let l = g.m.path_loops.get(&t).map_or(&[][..], |l| &l[..]);
    let mut v = Vec::new();
    for k in l {
        v.push(g.m.atoms[(*k)? as usize].clone());
    }
    v.sort();
    v.dedup();
    Some(v)
}

/// The path of `f`'s parameter path marker `t`, as the reader walks it.
pub(super) fn reach_key_of(
    g: &gen::Gen,
    f: model::FnIdx,
    t: model::Tok,
) -> Option<units::ReachKey> {
    if !g.m.is_path_marker(t) {
        return None;
    }
    let ks = g.m.path_keys(t);
    let (r, a0) = ks[0];
    let (p, _) = g.m.path_parent[&r];
    let &(_, i) = g.m.param_marker.get(&p)?;
    if g.m.marker_of.get(&(f, i)) != Some(&p) {
        return None;
    }
    let steps = ks[1..]
        .iter()
        .map(|&(x, k)| {
            (
                k.map(|a| g.m.atoms[a as usize].to_string()),
                path_loops(g, x),
            )
        })
        .collect();
    Some((i, g.m.atoms[a0? as usize].clone(), path_loops(g, r), steps))
}

#[path = "contracts/describe.rs"]
mod describe;
pub(crate) use describe::*;
/// Function `f`'s body may read its receiver: `this` or `super` in it (or
/// in a nested arrow), or a direct eval may (design §18.2: `ns.f()` passes
/// the namespace as the receiver of a function that never reads it). A
/// body not found among the module's functions may.
pub(super) fn reads_this(g: &gen::Gen, f: model::FnIdx) -> bool {
    let fi = &g.m.funcs[f as usize];
    if g.unit_in.is_some_and(|u| u.eval_scopes()) {
        return true;
    }
    let Some((_, module)) = g.modules.get(fi.module as usize) else {
        return true;
    };
    match module.functions.iter().find(|h| h.id == fi.func_id) {
        Some(h) => h.body.iter().any(gen_call::stmt_reads_this),
        None => true,
    }
}

/// What this unit's calls pass to closed function `nm` of another unit
/// (decision 13): per parameter of its stand-in, the value contract of the
/// arguments: the layouts of literal-shape objects of this unit, of
/// contract-class instances and of other units' stand-ins (forwarded), an
/// unknown value for anything else (a caller's own parameter marker
/// included: its callers pass anything, unless the caller is closed too). `top` when the import holds
/// anything but stand-in functions (⊤, a broken contract): its calls may
/// pass anything to the function.
/// A published class's closed key (design §18.13): its module path, name
/// and constructor parameter count, when the class is closed in its unit
/// (`UnitView::closed` holds its constructor) and the constructor reads no
/// `arguments` and has no rest parameter (a construction's extra arguments
/// reach nothing).
pub(super) fn closed_of(
    g: &gen::Gen,
    path: &str,
    mi: u32,
    tm: model::TmplId,
) -> Option<(String, String, u32)> {
    let f = g.m.tmpls[tm as usize].ctor?;
    let fi = &g.m.funcs[f as usize];
    let model::TmplKind::Class { name, .. } = &g.m.tmpls[tm as usize].kind else {
        return None;
    };
    let closed = g.view?.closed.get(name) == Some(&fi.func_id);
    (mi == 0 && closed && !fi.has_arguments && fi.rest.is_none())
        .then(|| (path.to_string(), name.clone(), fi.params.len() as u32))
}

/// DECISIONS 157: a class of this unit that is published
/// (`UnitView::published`): its holders forward its stand-in.
pub(super) fn forwards_of(g: &gen::Gen, mi: u32, tm: model::TmplId) -> bool {
    let model::TmplKind::Class { name, .. } = &g.m.tmpls[tm as usize].kind else {
        return false;
    };
    mi == 0 && g.view.is_some_and(|v| v.published.contains(name))
}

/// DECISIONS 199: another unit's layout contract, embedded in a value
/// contract, describes a layout once on its path (as DECISIONS 153 does for
/// this unit's own): one met again inside its own description keeps its
/// facts but not its fields' values. A forwarded copy read back as a
/// stand-in otherwise took one more level of itself every round
/// (d_react_ssr: two React literal shapes holding each other, the driver
/// never reached a fixpoint). The reader builds the outer description
/// first and finds it again by display (`Gen::stand_in_class`), so nothing
/// it reads is lost.
pub(crate) fn cut_described_again(cc: &mut units::ClassContract, path: &mut Vec<String>) {
    path.push(cc.display.clone());
    for (_, vc) in cc
        .field_values
        .iter_mut()
        .chain(cc.static_values.iter_mut())
    {
        for inner in vc.layouts.iter_mut().chain(vc.classes.iter_mut()) {
            if path.contains(&inner.display) {
                inner.field_values.clear();
                inner.static_values.clear();
            } else {
                cut_described_again(inner, path);
            }
        }
    }
    path.pop();
}

#[path = "contracts/values.rs"]
mod values;
pub(crate) use values::*;
/// DECISIONS 210: the layout of constructor function `f`'s instances (its
/// `new <f>` template `tm`) as the units constructing it hold it: its
/// facts, the methods on its prototype (contract functions among them with
/// their contracts) and the function's own contract as the constructor. A
/// prototype that is not the function's own (`f.prototype = ...`) or holds
/// anything but one function of this unit under a key leaves the keys
/// open: a reader finds unknown values there.
pub(super) fn fn_inst_contract(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    f: model::FnIdx,
    tm: model::TmplId,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> Option<units::ClassContract> {
    let facts = templates.iter().find(|t| t.id == tm)?.clone();
    let m = &g.m;
    let fi = &m.funcs[f as usize];
    let proto = m.tok_index.get(&model::TokKind::Proto(tm)).copied();
    let pa = m.atoms.iter().position(|x| x == "prototype");
    let ca = m.atoms.iter().position(|x| x == "constructor");
    let pf = pa.and_then(|a| m.fields.get(&(fi.token, a as model::Atom)).copied());
    let mut open = pf.is_some_and(|n| {
        let node = &m.nodes[n as usize];
        node.prim != 0 || node.pts.to_sorted().iter().any(|&p| Some(p) != proto)
    });
    let mut methods: Vec<(String, String)> = Vec::new();
    let mut method_fns: Vec<(String, units::FnContract)> = Vec::new();
    if let Some(proto) = proto {
        let mut own: Vec<(model::Atom, model::NodeId)> = m
            .field_keys
            .get(&proto)
            .into_iter()
            .flatten()
            .filter(|&&k| Some(k as usize) != ca)
            .map(|&k| (k, m.fields[&(proto, k)]))
            .collect();
        own.sort_unstable();
        for (k, n) in own {
            let node = &m.nodes[n as usize];
            let ts = node.pts.to_sorted();
            let fx = ts.first().map(|&t| m.tok_fn(t));
            match fx {
                Some(fx)
                    if ts.len() == 1
                        && node.prim == 0
                        && fx != model::NO_FN
                        && m.funcs[fx as usize].func_id != u32::MAX =>
                {
                    let key = m.atoms[k as usize].clone();
                    methods.push((key.clone(), m.funcs[fx as usize].name.clone()));
                    if m.contract_fns.contains(&fx) {
                        method_fns
                            .push((key, fn_contract_of(g, templates, fx, layouts_out, open_tm)));
                    }
                }
                // A key read through the chain but never written (a lookup
                // of an own field misses here): no member.
                _ if ts.is_empty() && node.prim == 0 => {}
                _ => open = true,
            }
        }
        // Any key: open when something wrote one (a read under an unknown
        // key leaves the node empty).
        if let Some(&n) = m.field_any.get(&proto) {
            let node = &m.nodes[n as usize];
            if node.prim != 0 || !node.pts.is_empty() {
                open = true;
            }
        }
    }
    let tpl = &m.tmpls[tm as usize];
    Some(units::ClassContract {
        display: tpl.display.clone(),
        name: fi.name.clone(),
        class_id: 0,
        literal_shape: false,
        facts,
        methods,
        getters: proto_getters(g, tm),
        own_getters: Vec::new(),
        native_parent: None,
        parent: None,
        field_fns: Vec::new(),
        field_layouts: nested_layouts(g, templates, tm, layouts_out, open_tm),
        field_values: field_value_contracts(g, templates, tm, layouts_out, open_tm),
        field_tas: field_ta_contracts(g, tm),
        open_keys: open_tm.contains(&tm) || open,
        statics: Vec::new(),
        statics_open: false,
        static_fns: Vec::new(),
        static_values: Vec::new(),
        closed: None,
        forwards: false,
        method_fns,
        ctor_fn: Some(fn_contract_of(g, templates, f, layouts_out, open_tm)),
        birth_keys: atoms_of(g, &tpl.birth_keys),
        definite: atoms_of(g, &tpl.definite),
        reserved: atoms_of(g, &tpl.reserved),
        ctor_leaks: true,
        ctor_returns: m.ctor_returns_object(f),
        binding_prim: 0,
        fn_inst: true,
    })
}

/// DECISIONS 152: `t` is what an argument marker reaches (its reach, or
/// its reach under a key, transitively), and a member of that argument
/// marker stands for values no contract describes: a parameter marker that
/// is no closed caller's (decision 13: its callers' values are beside it)
/// and has no values beside it (DECISIONS 142).
pub(super) fn arg_reach_unknown(g: &gen::Gen, t: model::Tok) -> bool {
    if !g.m.is_arg_marker(t) || !g.m.reach_marker.contains(&t) {
        return false;
    }
    let mut a = t;
    while g.m.reach_marker.contains(&a) {
        let up =
            g.m.marker_reach
                .iter()
                .find(|(_, &r)| r == a)
                .map(|(&p, _)| p)
                .or_else(|| {
                    g.m.keyed_reach
                        .iter()
                        .find(|(_, v)| v.iter().any(|&(_, r)| r == a))
                        .map(|(&p, _)| p)
                });
        match up {
            Some(p) => a = p,
            // No marker it is the reach of: nothing says what it holds.
            None => return true,
        }
    }
    g.m.marker_members(a).into_iter().any(|m| {
        !g.m.param_marker
            .get(&m)
            .is_some_and(|(f, _)| g.m.closed_params.contains_key(f))
            && !g.m.beside_values(m)
    })
}

pub(super) fn arg_contract_of(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    toks: Vec<model::Tok>,
    contracts_out: &[((String, String), units::ClassContract)],
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::ArgContract {
    let mut ac = units::ArgContract::default();
    if toks.is_empty()
        || !toks
            .iter()
            .all(|t| g.stand_in_fc.contains_key(t) || g.closed_ctors.iter().any(|c| c.2 == *t))
    {
        ac.top = true;
        return ac;
    }
    for t in toks {
        let fi = &g.m.funcs[g.m.tok_fn(t) as usize];
        // A closed class's stand-in constructor: its body reads no
        // `arguments` (`closed_of`); its parameters hand over what they
        // hold (`Gen::stand_in_class`).
        let closed_ctor = g.closed_ctors.iter().any(|c| c.2 == t);
        if fi.has_arguments && !closed_ctor {
            ac.top = true;
            continue;
        }
        if ac.params.len() < fi.params.len() {
            ac.params
                .resize(fi.params.len(), units::ValueContract::default());
        }
        for (i, &p) in fi.params.iter().enumerate() {
            let node = &g.m.nodes[p as usize];
            let vc = &mut ac.params[i];
            // A call passing fewer arguments passes undefined: every call of
            // the stand-in (direct, spread, bound, a builtin's callback)
            // binds through `bind_args`, which puts UNDEF in its missing
            // parameters, so the node's kinds already say so (DECISIONS 77).
            vc.prim |= node.prim;
            // An argument marker reads as the markers it stands for (DECISIONS 44).
            let toks = node.pts.to_sorted();
            // DECISIONS 152: what an argument marker reaches (`p.k`) has no
            // members of its own: it reads as what its members reach, a
            // value no contract describes here (as a parameter marker's
            // reach is), not as nothing.
            if toks.iter().any(|&t| arg_reach_unknown(g, t)) {
                vc.unknown_because("argument-marker-reach");
                vc.prim |= model::prim::ALL;
            }
            for t in toks.into_iter().flat_map(|t| g.m.marker_members(t)) {
                // A runtime-owned object, by name (`Model::named_shared`); a
                // builtin function value is unknown there besides.
                if let Some(s) = g.m.named_shared(t) {
                    let unknown_too = s == model::Shared::BuiltinFn;
                    if !vc.shared.contains(&s) {
                        vc.shared.push(s);
                    }
                    if !unknown_too {
                        continue;
                    }
                }
                // Another unit's function value (decision 19): by its
                // contract; the callee's calls of it publish what they pass
                // under its name. This unit's own function is unknown there
                // (it carries no markers unless published): passed to an
                // undescribed parameter it escapes, its parameters ⊤.
                if let model::TokKind::Func(f, _) = g.m.toks[t as usize] {
                    match g.stand_in_fc.get(&t) {
                        Some(&fc) if !vc.fns.iter().any(|c| c.name == fc.name) => {
                            vc.fns.push(forwarded_fn(g, fc))
                        }
                        Some(_) => {}
                        // Its closed publication (23), or a valid normal
                        // publication of an owned callable (223). The latter
                        // does not close its entry or erase any actual TOP.
                        // A non-publication escape remains undescribed here.
                        None if g.m.contract_fns.contains(&f)
                            && (g.m.closed_params.contains_key(&f)
                                && g.m.closed_pub.iter().any(|&u| g.m.tok_fn(u) == f)
                                // DEC223: ordinary publication proves callable
                                // identity, while its unclosed entry stays generic.
                                || g.m.escaped.get(&t).is_some_and(|r| {
                                    Some(*r) == g.m.pub_reason
                                        || Some(*r) == g.m.field_fn_reason
                                })) =>
                        {
                            let fc = fn_contract_of(g, templates, f, layouts_out, open_tm);
                            if !vc.fns.iter().any(|c| c.name == fc.name) {
                                vc.fns.push(fc);
                            }
                        }
                        None => vc.unknown_because("function-without-publication"),
                    }
                    continue;
                }
                let model::TokKind::Obj(tm, _) = g.m.toks[t as usize] else {
                    vc.unknown_because(match g.m.toks[t as usize] {
                        model::TokKind::Top | model::TokKind::BuiltinFn(_) => "top-argument",
                        model::TokKind::Bound(_) => "bound-function-argument",
                        model::TokKind::ClassObj(_) => "class-constructor-argument",
                        model::TokKind::Proto(_)
                        | model::TokKind::ObjectProto
                        | model::TokKind::NativeProto(_) => "prototype-argument",
                        model::TokKind::Builtin(_) => "builtin-argument",
                        model::TokKind::ModNs(_) => "module-namespace-argument",
                        model::TokKind::Func(..) | model::TokKind::Obj(..) => "argument-value",
                    });
                    continue;
                };
                if let Some((tm, kind)) = crate::bindings::sealed_owner(&g.m, t) {
                    let d = g.m.tmpls[tm as usize].display.clone();
                    if !g.unit_in.is_some_and(|u| u.broken(&format!("owner:{d}"))) {
                        vc.indexed_owners.push(crate::bindings::IndexedOwner {
                            kind,
                            min_len: *g.m.ta_min_len.get(&tm).unwrap_or(&0),
                            exact_len: g.m.ta_exact_len.get(&tm).copied(),
                            allocations: vec![d],
                        });
                        vc.indexed_owners.sort();
                        vc.indexed_owners.dedup();
                        continue;
                    }
                }
                // DECISIONS 149e: an array leaves by its element cell
                // (`Gen::contract_check`), named as a value contract names it (94).
                if g.m.is_cell_arr(t) {
                    let d = g.arr_name(t);
                    let v = if g.m.is_box(t) && !crate::derive::exact_array(&g.m, t) {
                        &mut vc.boxes
                    } else if g.m.is_box(t) {
                        &mut vc.arrayboxes
                    } else if crate::derive::exact_array(&g.m, t) {
                        &mut vc.arrays
                    } else {
                        &mut vc.made_arrays
                    };
                    if !v.contains(&d) {
                        v.push(d);
                        v.sort();
                    }
                    continue;
                }
                let display = &g.m.tmpls[tm as usize].display;
                // DECISIONS 152: a parameter marker handed to a closed
                // class's constructor: its function's contract hands it
                // over (`Model::handed`), and the callers' units publish
                // what they pass here.
                if closed_ctor
                    && g.m.param_marker.get(&t).is_some_and(|&(f, j)| {
                        j != u32::MAX && g.m.marker_of.get(&(f, j)) == Some(&t)
                    })
                {
                    continue;
                }
                // A closed caller passing its own parameter on: what its
                // callers pass is beside the marker (decision 13).
                if g.m
                    .param_marker
                    .get(&t)
                    .is_some_and(|(f, _)| g.m.closed_params.contains_key(f))
                {
                    continue;
                }
                // DECISIONS 142: a marker with values beside it (a published
                // method's receiver and what it reaches, DECISIONS 107) is
                // described by them, as a value contract's is (DECISIONS 137).
                if g.m.beside_values(t) {
                    continue;
                }
                if g.m.param_marker.contains_key(&t) || g.unit_in.is_some_and(|u| u.broken(display))
                {
                    vc.unknown_because(if g.m.param_marker.contains_key(&t) {
                        "parameter-marker"
                    } else {
                        "broken-layout"
                    });
                    // A marker is the whole argument (DECISIONS 119).
                    if g.m.param_marker.contains_key(&t) {
                        vc.prim |= model::prim::ALL;
                    }
                    continue;
                }
                let cc = if let Some(cc) = g.stand_in_cc.get(&tm) {
                    Some(cc.clone())
                } else if let Some((_, c)) =
                    contracts_out.iter().find(|(_, c)| &c.display == display)
                {
                    Some(c.clone())
                } else {
                    match (
                        templates.iter().find(|x| x.id == tm),
                        g.m.tmpls[tm as usize].kind.clone(),
                    ) {
                        (
                            Some(facts),
                            model::TmplKind::Class {
                                name,
                                class_id,
                                literal_shape: true,
                                ..
                            },
                        ) => Some(units::ClassContract {
                            display: display.clone(),
                            name,
                            class_id,
                            literal_shape: true,
                            facts: facts.clone(),
                            methods: Vec::new(),
                            getters: Vec::new(),
                            own_getters: own_getters(&g, tm),
                            native_parent: None,
                            parent: foreign_parent(&g, tm),
                            field_fns: Vec::new(),
                            field_layouts: Vec::new(),
                            field_values: Vec::new(),
                            field_tas: Vec::new(),
                            open_keys: open_tm.contains(&tm),
                            statics: Vec::new(),
                            statics_open: false,
                            static_fns: Vec::new(),
                            static_values: Vec::new(),
                            closed: None,
                            forwards: false,
                            method_fns: Vec::new(),
                            ctor_fn: None,
                            // The shape's own birth keys: a reader's stand-in judges
                            // birth fields and the CommonJS record marker on them
                            // (DECISIONS 101).
                            birth_keys: atoms_of(g, &g.m.tmpls[tm as usize].birth_keys),
                            definite: atoms_of(g, &g.m.tmpls[tm as usize].definite),
                            reserved: atoms_of(g, &g.m.tmpls[tm as usize].reserved),
                            ctor_leaks: true,
                            ctor_returns: g.chain_returns(tm),
                            binding_prim: 0,
                            fn_inst: false,
                        }),
                        _ => None,
                    }
                };
                match cc {
                    Some(mut cc) if !vc.layouts.iter().any(|c| c.display == cc.display) => {
                        // Its field functions, as a value contract's (`Gen::note_field_fns`).
                        if !g.stand_in_cc.contains_key(&tm) {
                            cc.field_fns = literal_field_fns(
                                g,
                                templates,
                                &node.pts.to_sorted(),
                                tm,
                                layouts_out,
                                open_tm,
                            );
                            // UEB: its fields as a published literal's
                            // (`Gen::nested_fields` decided them for the
                            // literals handed to a closed function,
                            // `Gen::handed`): what they hold, by key.
                            if cc.literal_shape
                                && !contracts_out.iter().any(|(_, c)| &c.display == display)
                            {
                                cc.field_layouts =
                                    nested_layouts(g, templates, tm, layouts_out, open_tm);
                                cc.field_values =
                                    field_value_contracts(g, templates, tm, layouts_out, open_tm);
                                cc.field_tas = field_ta_contracts(g, tm);
                            }
                        }
                        vc.layouts.push(cc)
                    }
                    Some(_) => {}
                    None => vc.unknown_because("layout-without-contract"),
                }
            }
        }
    }
    for p in &mut ac.params {
        p.shared.sort();
    }
    ac
}
