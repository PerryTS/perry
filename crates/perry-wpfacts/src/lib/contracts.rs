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
                .filter(|&&k| Some(k as usize) != pa)
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
                Some(&c) if g.m.funcs[f as usize].func_id == u32::MAX => c.clone(),
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
                    ctor_leaks: true,
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

pub(super) fn fn_contract_of(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    f: model::FnIdx,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::FnContract {
    let fi = &g.m.funcs[f as usize];
    // Its parameters carry markers (the contract states what the body does
    // with them); a contract function without them would claim it uses
    // nothing it is given.
    // A hidden capture parameter is no argument (DECISIONS 117): no marker.
    debug_assert!(
        (0..fi.params.len() as u32)
            .all(|i| g.m.marker_of.contains_key(&(f, i))
                || g.m.capture_param(f, i as usize).is_some()),
        "contract function {} has parameters without markers",
        fi.name
    );
    let uses = |reach: bool| -> Vec<u8> {
        (0..fi.params.len() as u32)
            .map(|i| {
                g.m.param_marker
                    .iter()
                    .filter(|&(t, &k)| k == (f, i) && g.m.reach_marker.contains(t) == reach)
                    .map(|(t, _)| g.m.marker_use.get(t).copied().unwrap_or(0))
                    .fold(0, |a, b| a | b)
            })
            .collect()
    };
    // A closed function's parameter every caller passes a described value
    // (decision 17): each is a stand-in here, its census (reports, store
    // effects) covers what this body does to it; the marker stands for
    // nothing, so no direct escape is published. What it reaches keeps
    // its uses. DECISIONS 148: a published method's parameter the running
    // units describe (`Model::method_params`) the same way.
    let described = g.m.closed_described.get(&f).or_else(|| {
        g.m.method_params
            .contains_key(&f)
            .then(|| g.m.method_described.get(&f))
            .flatten()
    });
    let params: Vec<u8> = uses(false)
        .into_iter()
        .enumerate()
        .map(|(i, u)| match described.and_then(|d| d.get(i)) {
            Some(true) => u & !model::MARK_ESCAPE,
            _ => u,
        })
        .collect();
    // What each parameter reaches, by the key it was read under (§18.4).
    let marker_use = |t: model::Tok| g.m.marker_use.get(&t).copied().unwrap_or(0);
    let reach: Vec<u8> = (0..fi.params.len() as u32)
        .map(|i| {
            let Some(&mk) = g.m.marker_of.get(&(f, i)) else {
                return 0;
            };
            g.m.marker_reach.get(&mk).map_or(0, |&r| marker_use(r))
        })
        .collect();
    let reach_keys: Vec<Vec<(String, u8)>> = (0..fi.params.len() as u32)
        .map(|i| {
            let Some(&mk) = g.m.marker_of.get(&(f, i)) else {
                return Vec::new();
            };
            let mut v: Vec<(model::Atom, u8)> =
                g.m.keyed_uses.get(&mk).cloned().unwrap_or_default();
            for &(a, r) in g.m.keyed_reach.get(&mk).into_iter().flatten() {
                match v.iter_mut().find(|(k, _)| *k == a) {
                    Some((_, u)) => *u |= marker_use(r),
                    None => v.push((a, marker_use(r))),
                }
            }
            // DECISIONS 126: a closed parameter every caller passes a
            // described value (decision 17): under a key where each such
            // value holds only stand-in typed arrays, the body reaches
            // nothing else there, and a typed array is a leaf whose census
            // (`Gen::reports`) covers every use here: no escape.
            let held = g.m.closed_params.get(&f).and_then(|ns| ns.get(i as usize));
            if let (Some(true), Some(&held)) = (
                g.m.closed_described
                    .get(&f)
                    .and_then(|d| d.get(i as usize))
                    .copied(),
                held,
            ) {
                for (a, u) in v.iter_mut() {
                    if described_ta_key(g, held, *a) {
                        *u &= !model::MARK_ESCAPE;
                    }
                }
            }
            let mut v: Vec<(String, u8)> = v
                .into_iter()
                .filter(|&(_, u)| u != 0)
                .map(|(a, u)| (g.m.atoms[a as usize].clone(), u))
                .collect();
            v.sort();
            v
        })
        .collect();
    let rets: Vec<model::NodeId> =
        g.m.contract_rets
            .iter()
            .filter(|(g2, _)| *g2 == f)
            .map(|&(_, n)| n)
            .collect();
    let d = describe_nodes(
        g,
        templates,
        f,
        rets,
        g.ret_top.contains(&f),
        layouts_out,
        open_tm,
    );
    // DECISIONS 140: the calls the body makes through a parameter marker,
    // each argument described as the result is.
    let mut param_calls: Vec<units::ParamCall> = Vec::new();
    for &(f2, i, key, call) in &g.m.marker_calls {
        if f2 != f {
            continue;
        }
        let args: Vec<units::Described> = g.m.calls[call as usize]
            .args
            .iter()
            .map(|&a| {
                let top = g.call_arg_top.contains(&a);
                describe_nodes(g, templates, f, vec![a], top, layouts_out, open_tm)
            })
            .collect();
        let key = key.map(|a| g.m.atoms[a as usize].clone());
        units::join_param_calls(
            &mut param_calls,
            &[units::ParamCall {
                param: i,
                key,
                args,
            }],
        );
    }
    // DECISIONS 152: the parameters' markers handed to a closed class's
    // constructor (`Model::handed`): the callers' units pass what they hold.
    let mut param_handovers: Vec<units::HandOverArg> = Vec::new();
    for &(t, ct, arg) in &g.m.handed {
        let Some((class, _, _)) = g.closed_ctors.iter().find(|c| c.2 == ct) else {
            continue;
        };
        for m in g.m.marker_members(t) {
            let own = g.m.param_marker.get(&m).is_some_and(|&(f2, i)| {
                f2 == f && i != u32::MAX && g.m.marker_of.get(&(f2, i)) == Some(&m)
            });
            if own {
                let param = g.m.param_marker[&m].1;
                param_handovers.push(units::HandOverArg {
                    param,
                    class: class.clone(),
                    arg,
                });
            }
        }
    }
    param_handovers.sort();
    param_handovers.dedup();
    // The receiver marker of a published method (DECISIONS 57); without
    // one nothing is known of what the body does to its receiver.
    let sealed = g.m.sealed_method(f);
    let (this_use, this_reach) = match g.m.marker_of.get(&(f, u32::MAX)) {
        // DECISIONS 116: a sealed class's method runs on this unit's own
        // instances only; what it does to them is in their facts.
        _ if sealed => (0, 0),
        Some(&t) => {
            let u = |t: model::Tok| g.m.marker_use.get(&t).copied().unwrap_or(0);
            (
                u(t),
                g.m.marker_reach
                    .get(&t)
                    .map_or(model::MARK_ESCAPE, |&r| u(r)),
            )
        }
        None => (model::MARK_ESCAPE, model::MARK_ESCAPE),
    };
    let this_stores = (sealed || !g.m.this_open.contains(&f)).then(|| {
        let mut ks: Vec<String> =
            g.m.this_stores
                .get(&f)
                .map(|ks| ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect())
                .unwrap_or_default();
        ks.sort();
        ks
    });
    let param_stores: Vec<Option<Vec<String>>> = (0..fi.params.len() as u32)
        .map(|i| {
            (!g.m.param_open.contains(&(f, i))).then(|| {
                let mut ks: Vec<String> =
                    g.m.param_stores
                        .get(&(f, i))
                        .map(|ks| ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect())
                        .unwrap_or_default();
                ks.sort();
                ks
            })
        })
        .collect();
    let mut param_flows: Vec<units::StoreFlow> =
        g.m.store_flows
            .iter()
            .filter(|((h, ..), _)| *h == f)
            .map(|(&(_, dst, reach, via, a), fl)| {
                // DECISIONS 176: what it stores of this unit; ⊤ once no
                // contract describes it.
                let value = match fl.vals {
                    Some(n) if !fl.vals_top => Some(describe_nodes(
                        g,
                        templates,
                        f,
                        vec![n],
                        false,
                        layouts_out,
                        open_tm,
                    )),
                    _ => None,
                };
                let mut from = fl.from.clone();
                from.sort();
                let mut from_reach: Vec<(u32, Option<String>)> = fl
                    .from_reach
                    .iter()
                    .map(|&(j, v)| (j, v.map(|v| g.m.atoms[v as usize].clone())))
                    .collect();
                from_reach.sort();
                units::StoreFlow {
                    dst,
                    reach,
                    via: via.map(|v| g.m.atoms[v as usize].clone()),
                    key: match a {
                        model::Key::Named(a) => Some(g.m.atoms[a as usize].clone()),
                        model::Key::Any => None,
                    },
                    from,
                    from_reach,
                    top: fl.top || (fl.vals.is_some() && fl.vals_top),
                    prim: fl.prim,
                    value,
                }
            })
            .collect();
    param_flows
        .sort_by(|a, b| (a.dst, a.reach, &a.via, &a.key).cmp(&(b.dst, b.reach, &b.via, &b.key)));
    let mut this_calls: Vec<String> =
        g.m.this_calls
            .get(&f)
            .map(|ks| ks.iter().map(|&a| g.m.atoms[a as usize].clone()).collect())
            .unwrap_or_default();
    this_calls.sort();
    let (name, closed) = fn_contract_name(g, f);
    // DECISIONS 162: the array cells it stores an argument into.
    let param_cells: Vec<(u32, String)> = g
        .cell_flows
        .iter()
        .filter(|(g_f, _, _)| *g_f == f)
        .map(|(_, i, k)| (*i, k.clone()))
        .collect();
    units::FnContract {
        method: g.m.published_this.contains_key(&f),
        this_class: this_class(g, f),
        this_use,
        this_reach,
        this_stores,
        this_calls,
        param_stores,
        param_flows,
        param_calls,
        param_handovers,
        param_cells,
        name,
        closed,
        params,
        reach,
        reach_keys,
        // A rest parameter takes only the arguments past the others
        // (design §18.11); `arguments` aliases every argument.
        variadic: fi.has_arguments,
        rest: fi.rest.is_some() && !fi.has_arguments,
        ret: d.layouts,
        ret_params: d.params,
        ret_reach: d.reach,
        ret_reach_keys: d.reach_keys,
        ret_unknown: d.unknown,
        ret_value: d.value,
        ret_prim: d.prim,
        lexical_this: fi.lexical_this || !reads_this(g, f),
    }
}

/// DECISIONS 147: function `f` of this unit named in a value it hands out.
/// The reader takes the owner's entry by the name (`Gen::owner_fn`,
/// DECISIONS 106); until the owner published one, this entry claims
/// nothing: every argument escapes, the result is unknown.
/// DECISIONS 165: this unit's published methods another unit cannot name:
/// a prototype key holding more than one function (the class contract
/// lists a method only alone under its key, `ClassContract::methods`) or a
/// computed member. A reader calls those through ⊤, which publishes no
/// run, so the owner publishes them as run itself: none reads its
/// parameter markers as having nothing beside them.
pub(super) fn unnamed_methods(m: &model::Model) -> impl Iterator<Item = model::FnIdx> + '_ {
    let proto = |h: model::Tok| matches!(m.toks[h as usize], model::TokKind::Proto(_));
    let fns = move |n: model::NodeId| {
        m.nodes[n as usize]
            .pts
            .to_sorted()
            .into_iter()
            .map(|t| m.tok_fn(t))
            .filter(|&f| f != model::NO_FN && m.method_described.contains_key(&f))
    };
    m.fields
        .iter()
        .filter(move |(&(h, _), &n)| proto(h) && m.nodes[n as usize].pts.to_sorted().len() > 1)
        .flat_map(move |(_, &n)| fns(n))
        .chain(
            m.field_any
                .iter()
                .filter(move |(&h, _)| proto(h))
                .flat_map(move |(_, &n)| fns(n)),
        )
}

/// DECISIONS 212: the one published class whose instances method `f` runs
/// on (its contract receivers, `Model::published_this`), by display.
fn this_class(g: &gen::Gen, f: model::FnIdx) -> Option<String> {
    let rs = g.m.published_this.get(&f)?;
    let mut ds = rs
        .iter()
        .filter_map(|r| g.m.recv_class.get(r))
        .map(|&tm| g.m.tmpls[tm as usize].display.clone());
    let d = ds.next()?;
    ds.all(|x| x == d).then_some(d)
}

pub(super) fn fn_named(g: &gen::Gen, f: model::FnIdx) -> units::FnContract {
    let fi = &g.m.funcs[f as usize];
    let n = fi.params.len();
    let (name, closed) = fn_contract_name(g, f);
    units::FnContract {
        method: g.m.published_this.contains_key(&f),
        this_class: this_class(g, f),
        name,
        closed,
        params: vec![model::MARK_ESCAPE; n],
        reach: vec![model::MARK_ESCAPE; n],
        reach_keys: vec![Vec::new(); n],
        variadic: fi.has_arguments,
        rest: fi.rest.is_some() && !fi.has_arguments,
        ret: None,
        ret_params: Vec::new(),
        ret_reach: Vec::new(),
        ret_reach_keys: Vec::new(),
        ret_unknown: true,
        ret_value: Default::default(),
        ret_prim: model::prim::ALL,
        lexical_this: fi.lexical_this,
        this_use: model::MARK_ESCAPE,
        this_reach: model::MARK_ESCAPE,
        this_stores: None,
        this_calls: Vec::new(),
        param_stores: vec![None; n],
        param_flows: Vec::new(),
        param_calls: Vec::new(),
        param_handovers: Vec::new(),
        param_cells: Vec::new(),
    }
}

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

/// What a value contract function `f` hands the caller's unit may be
/// (DECISIONS 140): its result, or an argument of a call through one of its
/// parameters; `nodes` hold it, `top` when no contract describes it.
pub(super) fn describe_nodes(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    f: model::FnIdx,
    nodes: Vec<model::NodeId>,
    top: bool,
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::Described {
    let mut ret_params: Vec<u32> = Vec::new();
    let mut ret_reach: Vec<u32> = Vec::new();
    let mut ret_reach_keys: Vec<units::ReachKey> = Vec::new();
    let keys_read = |t: model::Tok| keys_read(g, t);
    let join_keys = |a: Option<Vec<String>>, b: Option<Vec<String>>| -> Option<Vec<String>> {
        let mut v = a?;
        v.extend(b?);
        v.sort();
        v.dedup();
        Some(v)
    };
    let path_of = |t: model::Tok| reach_key_of(g, f, t);
    let mut ret_unknown = false;
    let mut ret_value = units::ValueContract::default();
    let mut ret_prim = nodes.iter().fold(0, |p, &n| p | g.m.nodes[n as usize].prim);
    let layouts = (!top && !nodes.is_empty()).then(|| {
        let mut ds: Vec<String> = Vec::new();
        for n in nodes {
            // An argument marker reads as the markers it stands for (DECISIONS 44).
            let mut toks: Vec<model::Tok> = Vec::new();
            for t in g.m.nodes[n as usize].pts.to_sorted() {
                // Its keyed reach is what each member reaches under the key,
                // then under the keys read on it: one named key of this
                // function's own parameter, or a deeper reach of it (a
                // keyed reach marker: its key, then the keys read on both,
                // DECISIONS 138; the any-key reach: what the parameter
                // reaches). Anything else (an escaped member, another
                // function's) reads as ⊤, another unit's value whole
                // (DECISIONS 119: no ⊤ beside the marker stands for it).
                if let Some(&(p, a)) = g.m.keyed_parent.get(&t) {
                    let ms = g.m.marker_members(p);
                    let a = g.m.atoms[a as usize].clone();
                    for &m in &ms {
                        // A member on a path of this function's own
                        // parameter: that path, then `a` (DECISIONS 158).
                        if let Some(mut k) = path_of(m).filter(|_| !g.m.marker_is_top(m)) {
                            k.3.push((Some(a.clone()), keys_read(t)));
                            if !ret_reach_keys.contains(&k) {
                                ret_reach_keys.push(k);
                            }
                            continue;
                        }
                        let own = match g.m.param_marker.get(&m) {
                            Some(&(f2, i)) if f2 == f && !g.m.marker_is_top(m) => {
                                g.m.marker_of.get(&(f, i)).map(|&mk| (i, mk))
                            }
                            _ => None,
                        };
                        let deeper = own.and_then(|(i, mk)| {
                            g.m.keyed_reach
                                .get(&mk)?
                                .iter()
                                .find(|&&(_, r)| r == m)
                                .map(|&(k0, _)| (i, k0))
                        });
                        match own {
                            Some((i, mk)) if mk == m => {
                                let k = (i, a.clone(), keys_read(t), Vec::new());
                                if !ret_reach_keys.contains(&k) {
                                    ret_reach_keys.push(k);
                                }
                            }
                            Some((i, _)) if deeper.is_some() => {
                                let k0 = deeper
                                    .map(|(_, k0)| g.m.atoms[k0 as usize].clone())
                                    .unwrap();
                                let ks = join_keys(
                                    join_keys(keys_read(m), keys_read(t)),
                                    Some(vec![a.clone()]),
                                );
                                let k = (i, k0, ks, Vec::new());
                                if !ret_reach_keys.contains(&k) {
                                    ret_reach_keys.push(k);
                                }
                            }
                            Some((i, mk)) if g.m.marker_reach.get(&mk) == Some(&m) => {
                                if !ret_reach.contains(&i) {
                                    ret_reach.push(i);
                                }
                            }
                            _ => {
                                ret_unknown = true;
                                ret_prim |= model::prim::ALL;
                            }
                        }
                    }
                    if ms.is_empty() {
                        ret_unknown = true;
                        ret_prim |= model::prim::ALL;
                    }
                    continue;
                }
                toks.extend(g.m.marker_members(t));
            }
            for t in toks {
                // An escaped marker reads as ⊤ (DECISIONS 42).
                if matches!(
                    g.m.toks[t as usize],
                    model::TokKind::Top | model::TokKind::BuiltinFn
                ) || g.m.marker_is_top(t)
                {
                    ret_unknown = true;
                    // An escaped marker is another unit's value whole
                    // (DECISIONS 119): any primitive too.
                    if g.m.is_marker(t) {
                        ret_prim |= model::prim::ALL;
                    }
                    continue;
                }
                // A value no contract describes escaped alone
                // (`Gen::publish_result`, DECISIONS 175): unknown.
                if !g.result_tok_ok(f, t) {
                    ret_unknown = true;
                    continue;
                }
                // A builtin, an array (DECISIONS 94), a primitive method's
                // container (184).
                let extra = match g.m.toks[t as usize] {
                    model::TokKind::Builtin(_) => true,
                    model::TokKind::Obj(..) => g.m.is_cell_arr(t),
                    _ => false,
                };
                if extra {
                    let vc =
                        value_contract_of_toks(g, templates, 0, vec![t], &[], layouts_out, open_tm);
                    ret_value.join(&vc);
                    continue;
                }
                // DECISIONS 147: a function it hands out, by name: the reader
                // calls it through its owner's entry (DECISIONS 106), a
                // function of this unit is one (`Gen::publish_result` made it
                // a contract function, `own_fns_out`); another unit's by its
                // stand-in contract.
                if let model::TokKind::Func(f2, _) = g.m.toks[t as usize] {
                    let fc = match g.stand_in_fc.get(&t) {
                        Some(&fc) => fc.clone(),
                        None if g.m.funcs[f2 as usize].func_id != u32::MAX => fn_named(g, f2),
                        None => {
                            ret_unknown = true;
                            continue;
                        }
                    };
                    if !ret_value.fns.iter().any(|c| c.name == fc.name) {
                        ret_value.fns.push(fc);
                    }
                    continue;
                }
                if let Some(k) = path_of(t) {
                    if !ret_reach_keys.contains(&k) {
                        ret_reach_keys.push(k);
                    }
                    continue;
                }
                if let Some(&(_, i)) = g.m.param_marker.get(&t) {
                    // What the parameter reaches through one named key: a
                    // keyed reach marker of the parameter's own marker.
                    let keyed =
                        g.m.marker_of
                            .get(&(f, i))
                            .and_then(|mk| g.m.keyed_reach.get(mk)?.iter().find(|&&(_, r)| r == t));
                    if let Some(&(a, _)) = keyed {
                        let k = (i, g.m.atoms[a as usize].clone(), keys_read(t), Vec::new());
                        if !ret_reach_keys.contains(&k) {
                            ret_reach_keys.push(k);
                        }
                        continue;
                    }
                    let v = if g.m.reach_marker.contains(&t) {
                        &mut ret_reach
                    } else {
                        &mut ret_params
                    };
                    if !v.contains(&i) {
                        v.push(i);
                    }
                    continue;
                }
                let Some(tm) = g.m.tmpl_of(t) else { continue };
                let display = g.m.tmpls[tm as usize].display.clone();
                if ds.contains(&display) {
                    continue;
                }
                // Another unit's layout (a stand-in: `return defaults`) is
                // forwarded with that unit's contract, its proof; a reader's
                // census report breaks it at its owner (§11.3).
                if let Some(cc) = g.stand_in_cc.get(&tm) {
                    if !layouts_out.iter().any(|c| c.display == display) {
                        layouts_out.push(cc.clone());
                    }
                    ds.push(display);
                    continue;
                }
                if let (
                    Some(facts),
                    model::TmplKind::Class {
                        name,
                        class_id,
                        literal_shape,
                        ..
                    },
                ) = (
                    templates.iter().find(|x| x.id == tm),
                    g.m.tmpls[tm as usize].kind.clone(),
                ) {
                    if !layouts_out.iter().any(|c| c.display == display) && !chain_unsettled(&g, tm)
                    {
                        layouts_out.push(units::ClassContract {
                            display: display.clone(),
                            name,
                            class_id,
                            literal_shape,
                            facts: facts.clone(),
                            methods: Vec::new(),
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
                            ctor_leaks: true,
                            binding_prim: 0,
                            fn_inst: false,
                        });
                        let nl = nested_layouts(g, templates, tm, layouts_out, open_tm);
                        let fv = field_value_contracts(g, templates, tm, layouts_out, open_tm);
                        if let Some(c) = layouts_out.iter_mut().find(|c| c.display == display) {
                            c.field_layouts = nl;
                            c.field_values = fv;
                            c.field_tas = field_ta_contracts(g, tm);
                        }
                        // Its field functions, as a value contract's (DECISIONS 78,
                        // `Gen::note_field_fns`). The layout is out first, so a field
                        // function returning this literal again does not recurse.
                        if literal_shape {
                            let nt = g.m.nodes[n as usize].pts.to_sorted();
                            let ff = literal_field_fns(g, templates, &nt, tm, layouts_out, open_tm);
                            if let Some(c) = layouts_out.iter_mut().find(|c| c.display == display) {
                                c.field_fns = ff;
                            }
                        }
                    }
                }
                ds.push(display);
            }
        }
        ds.sort();
        ds
    });
    ret_params.sort();
    ret_reach.sort();
    ret_reach_keys.sort();
    units::Described {
        layouts,
        params: ret_params,
        reach: ret_reach,
        reach_keys: ret_reach_keys,
        unknown: ret_unknown,
        value: ret_value,
        prim: ret_prim,
    }
}

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

/// The value contract of what a node may hold (design §12): the layouts and
/// classes of contracts, this unit's contract functions, other units'
/// stand-ins, builtins, primitive kinds; anything else is unknown.
pub(super) fn value_contract_of(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    n: model::NodeId,
    contracts_out: &[((String, String), units::ClassContract)],
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::ValueContract {
    let node = &g.m.nodes[n as usize];
    value_contract_of_toks(
        g,
        templates,
        node.prim,
        node.pts.to_sorted(),
        contracts_out,
        layouts_out,
        open_tm,
    )
}

/// `value_contract_of` over these values.
///
/// DECISIONS 208: a layout of this unit it describes stands in
/// `layouts_out` until the outermost value contract is built: met again
/// anywhere in it, it is named, not described again.
pub(super) fn value_contract_of_toks(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    prim: u16,
    toks: Vec<model::Tok>,
    contracts_out: &[((String, String), units::ClassContract)],
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::ValueContract {
    let outermost = g.describing.borrow().is_none();
    if outermost {
        *g.describing.borrow_mut() = Some(Vec::new());
    }
    let vc = describe_toks(
        g,
        templates,
        prim,
        toks,
        contracts_out,
        layouts_out,
        open_tm,
    );
    if outermost {
        for d in g.describing.take().unwrap_or_default() {
            if let Some(i) = layouts_out.iter().position(|c| c.display == d) {
                layouts_out.remove(i);
            }
        }
    }
    vc
}

pub(super) fn describe_toks(
    g: &gen::Gen,
    templates: &[TemplateFacts],
    prim: u16,
    toks: Vec<model::Tok>,
    contracts_out: &[((String, String), units::ClassContract)],
    layouts_out: &mut Vec<units::ClassContract>,
    open_tm: &pts::FxHashSet<model::TmplId>,
) -> units::ValueContract {
    let mut vc = units::ValueContract {
        prim,
        ..Default::default()
    };
    // DECISIONS 189: an escaped value beside ⊤ is that ⊤ (`Model::top_beside`:
    // unknown code holds it, a reader can use nothing it says). The solve
    // leaves one out only when ⊤ reached the node first, so a contract that
    // kept it would depend on the order its values arrived in: two units
    // writing one cell, each seeded with the other's write, swapped an
    // escaped array between their writes every round (tsc, `arr:` cell of
    // typescript.js#u4538, period 2) and the fixpoint never ended.
    let top = toks.contains(&model::TOP);
    let toks: Vec<model::Tok> = toks
        .into_iter()
        .filter(|&t| !(top && g.m.top_beside(t)))
        .collect();
    for &t in &toks {
        // A runtime-owned object, by name (`Model::named_shared`).
        if let Some(s) = g.m.named_shared(t) {
            if !vc.shared.contains(&s) {
                vc.shared.push(s);
            }
        }
        let (tm, class) = match g.m.toks[t as usize] {
            model::TokKind::Top | model::TokKind::BuiltinFn => {
                vc.unknown = true;
                continue;
            }
            model::TokKind::Builtin(ns) => {
                if !vc.builtins.contains(&ns) {
                    vc.builtins.push(ns);
                }
                continue;
            }
            // DECISIONS 137: a marker with values beside it (DECISIONS 74)
            // is described by them; it adds no kind of its own.
            model::TokKind::Obj(..) if g.m.is_marker(t) && g.m.beside_values(t) => continue,
            // An array (DECISIONS 94): its elements are cell `arr:<display>`.
            model::TokKind::Obj(..) if g.m.is_cell_arr(t) => {
                let d = g.arr_name(t);
                let v = if g.m.is_box(t) {
                    &mut vc.boxes
                } else {
                    &mut vc.arrays
                };
                if !v.contains(&d) {
                    v.push(d);
                    v.sort();
                }
                continue;
            }
            model::TokKind::Obj(tm, _) => (tm, false),
            model::TokKind::ClassObj(tm) => (tm, true),
            // A function of this unit that escaped before any contract
            // named it is no contract function: it left alone, and the
            // value says unknown beside the rest (DECISIONS 174, 178).
            model::TokKind::Func(f, _)
                if g.m.funcs[f as usize].func_id != u32::MAX && !g.m.contract_fns.contains(&f) =>
            {
                vc.unknown = true;
                continue;
            }
            model::TokKind::Func(f, _) if g.m.funcs[f as usize].func_id != u32::MAX => {
                // A contract function of this unit (`Gen::contract_check`).
                let fc = fn_contract_of(g, templates, f, layouts_out, open_tm);
                if !vc.fns.iter().any(|c| c.name == fc.name) {
                    vc.fns.push(fc);
                }
                continue;
            }
            _ => {
                if let Some(&fc) = g.stand_in_fc.get(&t) {
                    if !vc.fns.iter().any(|c| c.name == fc.name) {
                        vc.fns.push(fc.clone());
                    }
                }
                continue;
            }
        };
        let display = &g.m.tmpls[tm as usize].display;
        let cc = g
            .stand_in_cc
            .get(&tm)
            .cloned()
            .or_else(|| {
                contracts_out
                    .iter()
                    .find(|(_, c)| &c.display == display)
                    .map(|(_, c)| c.clone())
            })
            .or_else(|| {
                let facts = templates.iter().find(|x| x.id == tm)?.clone();
                if chain_unsettled(&g, tm) {
                    return None;
                }
                let (name, class_id, literal_shape) = match g.m.tmpls[tm as usize].kind.clone() {
                    model::TmplKind::Class {
                        name,
                        class_id,
                        literal_shape,
                        ..
                    } => (name, class_id, literal_shape),
                    // An enum's object (§18.2): a literal layout with
                    // no class (nothing guards it, so no class id).
                    // A plain object literal (DECISIONS 94) the same way.
                    model::TmplKind::Lit { .. } => (display.clone(), 0, true),
                    _ => return None,
                };
                Some(units::ClassContract {
                    display: display.clone(),
                    name,
                    class_id,
                    literal_shape,
                    facts,
                    methods: Vec::new(),
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
                    ctor_leaks: true,
                    binding_prim: 0,
                    fn_inst: false,
                })
            });
        let Some(mut cc) = cc else {
            vc.unknown = true;
            continue;
        };
        if g.stand_in_cc.contains_key(&tm) {
            cut_described_again(&mut cc, &mut Vec::new());
        }
        if !class
            && !g.stand_in_cc.contains_key(&tm)
            && !vc.layouts.iter().any(|c| c.display == cc.display)
        {
            // DECISIONS 153: a layout whose fields hold it again (a list's
            // or a tree's link beside forwarded stand-ins) is described once.
            // While its fields are described it stands in `layouts_out`, so
            // the description meeting it again takes that copy instead of
            // recursing; the reader builds the outer one first (its fields
            // hold the inner one) and finds it again by display
            // (`Gen::stand_in_class`). It leaves `layouts_out` afterwards: a
            // value contract's layout is not published. A published layout
            // is the reader's in any case (`Gen::other_layout`, DECISIONS 106).
            // DECISIONS 208: it stands there until the outermost value
            // contract is built (`value_contract_of_toks`), not only while
            // its own fields are described: a layout met again anywhere in
            // that contract takes the copy. Removed with its own description,
            // every sibling described it again, once per path: literal shapes
            // that hold one another (fast-check's regex tokens) made one
            // cell's contract 3.6 GB.
            // A layout found there is named, not described again: its facts,
            // no field contracts. Its description is the enclosing one or
            // the published one, which the reader takes by display before
            // any copy (DECISIONS 106); a published one copied whole carried
            // the copies its own fields held.
            match layouts_out.iter().find(|c| c.display == cc.display) {
                Some(p) => {
                    cc = units::ClassContract {
                        field_fns: Vec::new(),
                        field_layouts: Vec::new(),
                        field_values: Vec::new(),
                        field_tas: Vec::new(),
                        ..p.clone()
                    }
                }
                None => {
                    layouts_out.push(cc.clone());
                    if let Some(d) = g.describing.borrow_mut().as_mut() {
                        d.push(cc.display.clone());
                    }
                    cc.field_fns =
                        literal_field_fns(&g, templates, &toks, tm, layouts_out, open_tm);
                    cc.field_layouts = nested_layouts(g, templates, tm, layouts_out, open_tm);
                    cc.field_values = field_value_contracts(g, templates, tm, layouts_out, open_tm);
                    cc.field_tas = field_ta_contracts(g, tm);
                }
            }
        }
        let v = if class {
            &mut vc.classes
        } else {
            &mut vc.layouts
        };
        if !v.iter().any(|c| c.display == cc.display) {
            v.push(cc);
        }
    }
    vc.shared.sort();
    vc
}

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
        ctor_leaks: true,
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
                vc.unknown = true;
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
                            vc.fns.push(fc.clone())
                        }
                        Some(_) => {}
                        // This unit's closed function published through its
                        // closed contract (decision 23): its markers carry
                        // the body's uses; the callee publishes its calls.
                        // Markers and parameters are the function's, so one
                        // published token (another context's) covers it.
                        None if g.m.closed_params.contains_key(&f)
                            && g.m.contract_fns.contains(&f)
                            && g.m.closed_pub.iter().any(|&u| g.m.tok_fn(u) == f) =>
                        {
                            let fc = fn_contract_of(g, templates, f, layouts_out, open_tm);
                            if !vc.fns.iter().any(|c| c.name == fc.name) {
                                vc.fns.push(fc);
                            }
                        }
                        None => vc.unknown = true,
                    }
                    continue;
                }
                let model::TokKind::Obj(tm, _) = g.m.toks[t as usize] else {
                    vc.unknown = true;
                    continue;
                };
                // DECISIONS 149e: an array leaves by its element cell
                // (`Gen::contract_check`), named as a value contract names it (94).
                if g.m.is_cell_arr(t) {
                    let d = g.arr_name(t);
                    let v = if g.m.is_box(t) {
                        &mut vc.boxes
                    } else {
                        &mut vc.arrays
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
                    vc.unknown = true;
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
                            ctor_leaks: true,
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
                        }
                        vc.layouts.push(cc)
                    }
                    Some(_) => {}
                    None => vc.unknown = true,
                }
            }
        }
    }
    for p in &mut ac.params {
        p.shared.sort();
    }
    ac
}
