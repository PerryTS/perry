//! The contract of a function of the unit: its name, forwarded and compiled bodies, and the methods no key names.

use super::*;

pub(super) fn compiled_body_of(g: &gen::Gen, f: model::FnIdx) -> Option<(String, u32, bool)> {
    let fi = &g.m.funcs[f as usize];
    if fi.func_id == u32::MAX {
        return g.stand_in_fc.get(&fi.token)?.compiled_body.clone();
    }
    let closure = g.m.closures.contains(&f);
    (closure || g.m.decls.contains(&f)).then(|| {
        (
            g.modules[fi.module as usize].0.to_owned(),
            fi.func_id,
            closure,
        )
    })
}

/// Another unit's function `fc` (a stand-in's contract) as this unit
/// forwards it: whole, with the functions described inside it that have an
/// owner's entry named (`units::name_owned_in_fn`, DECISIONS 106).
pub(crate) fn forwarded_fn(g: &gen::Gen, fc: &units::FnContract) -> units::FnContract {
    let mut c = fc.clone();
    if let Some(u) = g.unit_in {
        units::name_owned_in_fn(&mut c, &|n| u.own_fn(n).is_some());
    }
    c
}

pub(crate) fn fn_contract_of(
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
    // units describe (`Model::method_params`) the same way. DECISIONS 224:
    // so a function published to a known closed callee, also before any
    // unit runs it (DECISIONS 165: its markers stand for nothing): a
    // runner's value reaches it through the argument join, as a stand-in
    // whose census covers what this body does to it. Unknown at a position
    // (a runner passes what no contract describes), the marker keeps its
    // uses.
    let described = g.m.closed_described.get(&f).or_else(|| {
        (g.m.method_params.contains_key(&f) || g.m.callable_pub.contains(&f))
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
            // DECISIONS 242 (contract half): every key the body reads on
            // the argument (a miss site where the argument does not own
            // it), `STAR_KEY` for a computed key; a key with uses is a
            // read too.
            for &k in g.m.param_reads.get(&mk).into_iter().flatten() {
                let k = match k {
                    model::Key::Named(a) => g.m.atoms[a as usize].clone(),
                    model::Key::Any(_) => crate::STAR_KEY.to_string(),
                };
                if !v.iter().any(|(x, _)| *x == k) {
                    v.push((k, 0));
                }
            }
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
    let mut d = describe_nodes(
        g,
        templates,
        f,
        rets,
        g.ret_top.contains(&f),
        layouts_out,
        open_tm,
    );
    // A ⊤ result has one spelling: `ret: None` with `ret_unknown` and every
    // primitive kind, as `fn_named` describes one. A reader takes a result named
    // by no contract (`None`) and one whose only member is ⊤ (`Some([])`,
    // unknown, every kind) the same way (`Gen::fill_described`: ⊤ and every
    // kind), but the two digests differ, and which one a solve publishes
    // depends on the order its escapes ran (`ret_top` holds only a result
    // that escaped before its function became a contract function). Readers
    // embed the contract in what they publish, so its owner solved again and
    // published the other spelling: oxfmt's esbuild `__commonJS` closures in
    // the Supabase CLI alternated between the two every round, forever.
    if d.layouts.as_ref().is_some_and(|l| l.is_empty()) && d.unknown && d.prim == model::prim::ALL {
        d.layouts = None;
    }
    if d.layouts.is_none() {
        d.unknown = true;
        d.prim = model::prim::ALL;
    }
    // DECISIONS 140: the calls the body makes through a parameter marker,
    // each argument described as the result is.
    let mut param_calls: Vec<units::ParamCall> = Vec::new();
    for &(f2, i, path, key, call) in &g.m.marker_calls {
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
        // The receiver the body called it on (`o.k()`, `o` this unit's
        // value with the marker under `k`), described as the arguments are;
        // a bare call has none.
        let this = match g.m.calls[call as usize].this {
            model::ThisArg::Node(n) if key.is_none() && path.is_none() => Some(describe_nodes(
                g,
                templates,
                f,
                vec![n],
                g.call_arg_top.contains(&n),
                layouts_out,
                open_tm,
            )),
            _ => None,
        };
        let key = key.map(|a| g.m.atoms[a as usize].clone());
        let path = path.map(|a| g.m.atoms[a as usize].clone());
        units::join_param_calls(
            &mut param_calls,
            &[units::ParamCall {
                param: i,
                path,
                key,
                this,
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
                        model::Key::Any(_) => None,
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
    // DECISIONS 242 (contract half): the defineProperty calls on each
    // argument, run by the caller on it.
    let mut param_defines: Vec<(u32, Option<String>, bool)> = Vec::new();
    for i in 0..fi.params.len() as u32 {
        let Some(&mk) = g.m.marker_of.get(&(f, i)) else {
            continue;
        };
        for &(k, data) in g.m.param_defines.get(&mk).into_iter().flatten() {
            param_defines.push((i, k.map(|a| g.m.atoms[a as usize].clone()), data));
        }
    }
    param_defines.sort();
    param_defines.dedup();
    // DECISIONS 162: the array cells it stores an argument into.
    let param_cells: Vec<(u32, String)> = g
        .cell_flows
        .iter()
        .filter(|(g_f, _, _)| *g_f == f)
        .map(|(_, i, k)| (*i, k.clone()))
        .collect();
    units::FnContract {
        compiled_body: compiled_body_of(g, f),
        method: g.m.published_this.contains_key(&f),
        callable: g.m.callable_pub.contains(&f),
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
        param_defines,
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
pub(crate) fn unnamed_methods(m: &model::Model) -> impl Iterator<Item = model::FnIdx> + '_ {
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
pub(super) fn this_class(g: &gen::Gen, f: model::FnIdx) -> Option<String> {
    let rs = g.m.published_this.get(&f)?;
    let mut ds = rs
        .iter()
        .filter_map(|r| g.m.recv_class.get(r))
        .map(|&tm| g.m.tmpls[tm as usize].display.clone());
    let d = ds.next()?;
    ds.all(|x| x == d).then_some(d)
}

pub(crate) fn fn_named(g: &gen::Gen, f: model::FnIdx) -> units::FnContract {
    let fi = &g.m.funcs[f as usize];
    let n = fi.params.len();
    let (name, closed) = fn_contract_name(g, f);
    units::FnContract {
        compiled_body: compiled_body_of(g, f),
        method: g.m.published_this.contains_key(&f),
        callable: g.m.callable_pub.contains(&f),
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
        param_defines: Vec::new(),
    }
}
