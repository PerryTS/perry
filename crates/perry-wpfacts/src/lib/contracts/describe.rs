//! Describing the nodes that hold a function result or an argument of a call through a parameter.

use super::*;

/// What a value contract function `f` hands the caller's unit may be
/// (DECISIONS 140): its result, or an argument of a call through one of its
/// parameters; `nodes` hold it, `top` when no contract describes it.
pub(crate) fn describe_nodes(
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
            // DEC189 applies to described results/callback arguments too:
            // an escaped concrete value beside TOP is subsumed by TOP,
            // independently of which reached this node first. Keep every
            // marker and actual TOP, and retain private/known-only values.
            // A function of this unit published through its contract only
            // (an export, a published field, a contract result: DECISIONS
            // 147/210) is not held by unknown code: the reader calls it
            // through that contract and any other use is its census report
            // (DECISIONS 209). Subsumed, the reader would hold it as ⊤ and
            // report nothing while its owner reads it as published.
            let unknown = toks.iter().any(|&t| {
                matches!(
                    g.m.toks[t as usize],
                    model::TokKind::Top | model::TokKind::BuiltinFn(_)
                ) || g.m.marker_is_top(t)
            });
            let contract_only = |t: model::Tok| match g.m.toks[t as usize] {
                model::TokKind::Func(f, _) => {
                    g.m.funcs[f as usize].func_id != u32::MAX
                        && g.m.contract_fns.contains(&f)
                        && g.m.escaped.get(&t).is_some_and(|&r| {
                            Some(r) == g.m.pub_reason || Some(r) == g.m.field_fn_reason
                        })
                }
                _ => false,
            };
            for t in toks {
                if unknown && g.m.top_beside(t) && !contract_only(t) {
                    continue;
                }
                // An escaped marker reads as ⊤ (DECISIONS 42).
                if matches!(
                    g.m.toks[t as usize],
                    model::TokKind::Top | model::TokKind::BuiltinFn(_)
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
                        Some(&fc) => forwarded_fn(g, fc),
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
