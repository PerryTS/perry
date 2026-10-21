//! TEMP CWL census (profile only, `CWL_CENSUS=<unit path substring>`):
//! instances by (key kind, function, receiver layout); object and closure
//! tokens by template/function and context. Removed at the landing split.
use crate::model::*;
use crate::pts::{FxHashMap, FxHashSet};

fn layout(m: &Model, t: Tok) -> String {
    match &m.toks[t as usize] {
        TokKind::Obj(tm, _) => format!("T{tm}"),
        TokKind::ClassObj(tm) => format!("C{tm}"),
        TokKind::Proto(tm) => format!("P{tm}"),
        TokKind::Top => "TOP".into(),
        TokKind::Func(f, _) => format!("F{f}"),
        TokKind::Bound(_) => "bound".into(),
        TokKind::ObjectProto => "OP".into(),
        TokKind::Builtin(b) => format!("B{b:?}"),
        TokKind::BuiltinFn => "bf".into(),
        TokKind::ModNs(x) => format!("ns{x}"),
        TokKind::NativeProto(n) => format!("np{n}"),
    }
}

pub fn run(m: &Model, unit: &str) {
    let s = &m.sum;
    let mut key_of: Vec<(u8, u32, Tok)> = vec![(255, 0, 0); s.insts.len()];
    for (&k, &i) in &s.inst_key {
        key_of[i as usize] = k;
    }
    let n_prog = s.n_prog as u32;
    // Per kind group: 0 program call, 6 site, 5 shared, 2 entry, 3/4 group.
    #[derive(Default)]
    struct G {
        n: u64,
        site_ft: FxHashSet<(u32, Tok)>,
        site_f: FxHashSet<(u32, FnIdx)>,
        ft: FxHashSet<Tok>,
        f: FxHashSet<FnIdx>,
        lset_f: FxHashSet<(String, FnIdx)>,
        lay_f: FxHashSet<(String, FnIdx)>,
        nothis_f: FxHashSet<FnIdx>,
        site_tmpl: u64,
        site_prog: u64,
        lsize: [u64; 6],
    }
    let mut g: FxHashMap<u8, G> = FxHashMap::default();
    #[derive(Default)]
    struct PerF {
        n: u64,
        sites: FxHashSet<u32>,
        ft: FxHashSet<Tok>,
        lsets: FxHashSet<String>,
        lays: FxHashSet<String>,
        kinds: [u64; 7],
    }
    let mut perf: FxHashMap<FnIdx, PerF> = FxHashMap::default();
    let mut all_lay_f: FxHashSet<(String, FnIdx)> = FxHashSet::default();
    let mut all_nothis: FxHashSet<FnIdx> = FxHashSet::default();
    for (i, inst) in s.insts.iter().enumerate() {
        let (k, site, ft) = key_of[i];
        let f = inst.f;
        let mut lays: Vec<String> = Vec::new();
        let has_this = inst.this != NO_NODE;
        if has_this {
            let node = &m.nodes[inst.this as usize];
            let mut set: FxHashSet<String> = node.pts.to_sorted().iter().map(|&t| layout(m, t)).collect();
            if node.prim != 0 {
                set.insert("prim".into());
            }
            lays = set.into_iter().collect();
            lays.sort();
        }
        let lset = if has_this { lays.join(",") } else { "none".into() };
        let e = g.entry(k).or_default();
        e.n += 1;
        e.site_ft.insert((site, ft));
        e.site_f.insert((site, f));
        e.ft.insert(ft);
        e.f.insert(f);
        if k == 6 {
            if site < n_prog { e.site_prog += 1 } else { e.site_tmpl += 1 }
        }
        if has_this {
            e.lset_f.insert((lset.clone(), f));
            for l in &lays {
                e.lay_f.insert((l.clone(), f));
                all_lay_f.insert((l.clone(), f));
            }
            e.lsize[lays.len().min(5)] += 1;
        } else {
            e.nothis_f.insert(f);
            all_nothis.insert(f);
        }
        let p = perf.entry(f).or_default();
        p.n += 1;
        p.sites.insert(site);
        p.ft.insert(ft);
        p.lsets.insert(lset);
        for l in lays {
            p.lays.insert(l);
        }
        p.kinds[(k as usize).min(6)] += 1;
    }
    let mut ks: Vec<_> = g.keys().copied().collect();
    ks.sort();
    eprintln!("CWL unit={unit} instances={} n_prog={n_prog} calls={} nodes={} toks={} work={}", s.insts.len(), m.calls.len(), m.nodes.len(), m.toks.len(), m.work);
    for k in ks {
        let e = &g[&k];
        eprintln!(
            "CWL kind={k} n={} (site,ft)={} (site,f)={} ft={} f={} (lset,f)={} (layout,f)={} nothis_f={} site_is_prog={} site_is_tmpl={} lsize0..5+={:?}",
            e.n, e.site_ft.len(), e.site_f.len(), e.ft.len(), e.f.len(), e.lset_f.len(), e.lay_f.len(), e.nothis_f.len(), e.site_prog, e.site_tmpl, e.lsize
        );
    }
    eprintln!("CWL layoutkey all kinds: (layout,f)={} nothis_f={} => instances under the layout key ~{}", all_lay_f.len(), all_nothis.len(), all_lay_f.len() + all_nothis.len());
    // Nodes, pts and work attributable to instance slot nodes, per kind.
    let mut nk: FxHashMap<u32, (u64, u64, u64)> = FxHashMap::default();
    for (&n, &(_, kk)) in &m.cw9_inst_fn {
        let e = nk.entry(kk >> 28).or_default();
        e.0 += 1;
        e.1 += m.nodes[n as usize].pts.len() as u64;
        e.2 += m.cw9_node_work.get(&n).copied().unwrap_or(0);
    }
    let tot_w: u64 = m.cw9_node_work.values().sum();
    let tot_p: u64 = m.nodes.iter().map(|n| n.pts.len() as u64).sum();
    let mut nkv: Vec<_> = nk.into_iter().collect();
    nkv.sort();
    for (k, (n, p, w)) in nkv {
        eprintln!("CWL slotnodes kind={k} nodes={n} pts={p} work={w}");
    }
    eprintln!("CWL totals nodes={} pts={tot_p} nodework={tot_w}", m.nodes.len());
    let mut pv: Vec<_> = perf.iter().collect();
    pv.sort_by_key(|x| std::cmp::Reverse(x.1.n));
    for (f, p) in pv.iter().take(20) {
        let sm = s.of.get(f);
        eprintln!(
            "CWL fn {} inst={} kinds(0,2,3,4,5,6)={},{},{},{},{},{} sites={} ft={} lsets={} layouts={} nslots={} calls={} this={}",
            m.funcs[**f as usize].name, p.n, p.kinds[0], p.kinds[2], p.kinds[3], p.kinds[4], p.kinds[5], p.kinds[6],
            p.sites.len(), p.ft.len(), p.lsets.len(), p.lays.len(),
            sm.map_or(0, |x| x.nslots), sm.map_or(0, |x| x.calls.len()), sm.is_some_and(|x| x.this != NO_NODE)
        );
    }
    // Object tokens by template and context.
    #[derive(Default)]
    struct T {
        toks: u64,
        plain: u64,
        prog: u64,
        ctx: u64,
        other: u64,
        srcs: FxHashSet<CallId>,
        hosts: FxHashSet<CallId>,
    }
    let mut tm: FxHashMap<TmplId, T> = FxHashMap::default();
    let mut fc: FxHashMap<FnIdx, u64> = FxHashMap::default();
    let (mut nobj, mut nfn) = (0u64, 0u64);
    for t in &m.toks {
        match *t {
            TokKind::Obj(x, c) => {
                nobj += 1;
                let e = tm.entry(x).or_default();
                e.toks += 1;
                if c == NO_CTX {
                    e.plain += 1;
                } else if c < n_prog {
                    e.prog += 1;
                    e.srcs.insert(c);
                } else if c == crate::summary::BORN {
                    e.ctx += 1;
                } else {
                    e.other += 1;
                }
            }
            TokKind::Func(f, _) => {
                nfn += 1;
                *fc.entry(f).or_default() += 1;
            }
            _ => {}
        }
    }
    eprintln!("CWL objtoks={nobj} templates={} fntoks={nfn} fns={} closurectx={}", tm.len(), fc.len(), s.ctx_nodes.len());
    let mut tv: Vec<_> = tm.iter().collect();
    tv.sort_by_key(|x| std::cmp::Reverse(x.1.toks));
    for (x, e) in tv.iter().take(15) {
        eprintln!(
            "CWL tmpl {} toks={} plain={} prog={} ctx={} other={} srcs={} hosts={}",
            m.tmpls[**x as usize].display, e.toks, e.plain, e.prog, e.ctx, e.other, e.srcs.len(), e.hosts.len()
        );
    }
    let mut wv: Vec<_> = m.cwl_thisinst.iter().collect();
    wv.sort_by_key(|x| std::cmp::Reverse(*x.1));
    for ((why, wf), n) in wv.iter().take(10) { eprintln!("CWL thisinst why={why} fn={} n={n}", m.funcs[*wf as usize].name); }
    let mut rv: Vec<_> = m.cwl_recv.iter().collect();
    rv.sort_by_key(|x| std::cmp::Reverse(x.1 .0));
    for ((f, shape, lbl, prot, bs, prog), (n, first, site)) in rv.iter().take(25) {
        eprintln!("CWL recv fn={} this={shape} ret={lbl} protocol={prot} boundsynth={bs} progcall={prog} n={n} first={first} site={site}", m.funcs[*f as usize].name);
    }
    let mut fv: Vec<_> = fc.into_iter().collect();
    fv.sort_by_key(|x| std::cmp::Reverse(x.1));
    // CWL census: the enclosing chain of a function (who makes its contexts).
    let chain = |f: crate::model::FnIdx| -> Vec<String> {
        let mut out = Vec::new();
        let mut c = m.funcs[f as usize].parent;
        while c != crate::model::NO_FN && out.len() < 6 {
            out.push(m.funcs[c as usize].name.clone());
            c = m.funcs[c as usize].parent;
        }
        out
    };
    for (f, c) in fv.iter().take(10) {
        eprintln!("CWL fntok {} ctxs={c} chain={:?}", m.funcs[*f as usize].name, chain(*f));
    }
    // CWL census: instances and contexts per outermost enclosing function.
    let mut by_root: FxHashMap<String, (usize, usize, usize)> = FxHashMap::default();
    for (f, c) in &fv {
        let ch = chain(*f);
        let root = ch.last().cloned().unwrap_or_else(|| m.funcs[*f as usize].name.clone());
        let e = by_root.entry(root).or_default();
        e.0 += 1;
        e.1 += *c as usize;
    }
    for i in &s.insts {
        let ch = chain(i.f);
        let root = ch.last().cloned().unwrap_or_else(|| m.funcs[i.f as usize].name.clone());
        by_root.entry(root).or_default().2 += 1;
    }
    let mut br: Vec<_> = by_root.into_iter().collect();
    br.sort_by_key(|x| std::cmp::Reverse(x.1 .2));
    for (r, (nf, nc, ni)) in br.iter().take(15) {
        eprintln!("CWL root {r} closures={nf} ctxs={nc} instances={ni}");
    }
}
