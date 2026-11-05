//! The value contract of what a node or a token set may hold.

use super::*;

/// The value contract of what a node may hold (design §12): the layouts and
/// classes of contracts, this unit's contract functions, other units'
/// stand-ins, builtins, primitive kinds; anything else is unknown.
pub(crate) fn value_contract_of(
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
pub(crate) fn value_contract_of_toks(
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

pub(crate) fn describe_toks(
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
        let (tm, class) = match g.m.toks[t as usize] {
            model::TokKind::Top | model::TokKind::BuiltinFn(_) => {
                vc.unknown_because("top-value");
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
            model::TokKind::Obj(tm, _) => (tm, false),
            model::TokKind::ClassObj(tm) => (tm, true),
            // A function of this unit that escaped before any contract
            // named it is no contract function: it left alone, and the
            // value says unknown beside the rest (DECISIONS 174, 178).
            model::TokKind::Func(f, _)
                if g.m.funcs[f as usize].func_id != u32::MAX && !g.m.contract_fns.contains(&f) =>
            {
                vc.unknown_because("escaped-function-without-contract");
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
                        vc.fns.push(forwarded_fn(g, fc));
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
                })
            });
        let Some(mut cc) = cc else {
            vc.unknown_because("class-without-contract");
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
                    cc.field_fns =
                        literal_field_fns(&g, templates, &toks, tm, layouts_out, open_tm);
                    cc.field_layouts = nested_layouts(g, templates, tm, layouts_out, open_tm);
                    cc.field_values = field_value_contracts(g, templates, tm, layouts_out, open_tm);
                    cc.field_tas = field_ta_contracts(g, tm);
                    // A literal layout of this unit goes out under its
                    // display, described: the owner's entry, which a reader
                    // takes over any copy (`Gen::other_layout`, DECISIONS 106;
                    // a class goes out as DECISIONS 132 says). Without it the
                    // only entry is a forwarder's copy of an earlier round,
                    // which the forwarder itself reads back and forwards again.
                    match cc.literal_shape {
                        true => {
                            if let Some(e) =
                                layouts_out.iter_mut().find(|c| c.display == cc.display)
                            {
                                *e = cc.clone();
                            }
                        }
                        false => {
                            if let Some(d) = g.describing.borrow_mut().as_mut() {
                                d.push(cc.display.clone());
                            }
                        }
                    }
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
