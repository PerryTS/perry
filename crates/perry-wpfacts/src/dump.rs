//! Output: the facts dump (`PERRY_FACTS_DUMP`), the per-module digest, the
//! contract-build claims section, the sabotage control.

use crate::derive::{TemplateFacts, Verdict};
use crate::ProgramFacts;

pub(crate) const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub(crate) fn fnv(h: &mut u64, s: &str) {
    for b in s.as_bytes() {
        *h ^= *b as u64;
        *h = h.wrapping_mul(FNV_PRIME);
    }
    *h ^= 0xff;
    *h = h.wrapping_mul(FNV_PRIME);
}

fn b(v: bool) -> u8 {
    v as u8
}

pub(crate) fn template_lines(t: &TemplateFacts) -> Vec<String> {
    let mut v = vec![format!(
        "T {} kind={} class_id={} prefix_stable={} fixed_layout={} adds_keys={} birth=[{}] escape=\"{}\" hard=\"{}\" chain=\"{}\" statics=[{}]",
        t.display,
        t.kind,
        t.class_id.map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
        b(t.prefix_stable),
        b(t.fixed_layout),
        t.adds_keys,
        t.birth_keys.join(","),
        t.escape.clone().unwrap_or_default(),
        t.hard.clone().unwrap_or_default(),
        t.chain.clone().unwrap_or_default(),
        t.statics.join(","),
    )];
    for f in &t.fields {
        v.push(format!(
            "  F {} slot={} definite={} data={} readonly={} writers={}/{} kind={} fixed_kind={} ta_stable={}",
            crate::model::esc_key(&f.key),
            f.slot.map(|s| s.to_string()).unwrap_or_else(|| "-".into()),
            b(f.definite),
            b(f.data_field),
            b(f.readonly),
            f.writers_ctor,
            f.writers_post,
            f.kind.desc(),
            b(f.kind.fixed()),
            b(f.ta_stable),
        ));
    }
    v
}

fn fact_lines(t: &TemplateFacts) -> Vec<String> {
    let mut v = vec![
        format!(
            "FACT F3 prefix_stable {} = {}",
            t.display,
            b(t.prefix_stable)
        ),
        format!("FACT F3 fixed_layout {} = {}", t.display, b(t.fixed_layout)),
        format!(
            "FACT F3 escape {} = {}",
            t.display,
            b(t.escape.is_some() || t.hard.is_some())
        ),
    ];
    for f in &t.fields {
        v.push(format!(
            "FACT F1 readonly {}.{} = {}",
            t.display,
            crate::model::esc_key(&f.key),
            b(f.readonly)
        ));
        v.push(format!(
            "FACT F1' definite {}.{} = {}",
            t.display,
            crate::model::esc_key(&f.key),
            b(f.definite)
        ));
        v.push(format!(
            "FACT F2 kind {}.{} = {}",
            t.display,
            crate::model::esc_key(&f.key),
            f.kind.desc()
        ));
        v.push(format!(
            "FACT F6 ta_stable {}.{} = {}",
            t.display,
            crate::model::esc_key(&f.key),
            b(f.ta_stable)
        ));
    }
    v
}

pub fn dump_text(pf: &ProgramFacts) -> String {
    let s = &pf.stats;
    let mut out = String::new();
    out.push_str("# wpfacts v1\n");
    out.push_str(&format!(
        "# dynamic_code_reach {:?}\n",
        pf.dynamic_code_reach
    ));
    out.push_str(&format!(
        "# stats modules={} templates={} fact_units={} nodes={} constraints={} tokens={} functions={} gen_ms={:.1} solve_ms={:.1} derive_ms={:.1} pops={} work={} aborted={}\n",
        s.modules, s.templates, s.fact_units, s.nodes, s.constraints, s.tokens, s.functions, s.gen_ms, s.solve_ms, s.derive_ms, s.pops, s.work, s.aborted
    ));
    if pf.rounds > 0 {
        out.push_str(&format!(
            "# rounds {} work_per_round={:?} broken={:?}\n",
            pf.rounds, pf.round_work, pf.broken
        ));
    }
    for (path, u) in &pf.units {
        out.push_str(&format!(
            "# unit {path} nodes={} constraints={} tokens={} pts_entries={} instances={} gen_ms={:.1} solve_ms={:.1} derive_ms={:.1} pops={} work={} aborted={}\n",
            u.nodes, u.constraints, u.tokens, u.pts_entries, u.instances, u.gen_ms, u.solve_ms, u.derive_ms, u.pops, u.work, u.aborted
        ));
    }
    for l in &pf.unit_io {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str(&format!(
        "# interp_guard {}\n",
        pf.interp_guard.clone().unwrap_or_else(|| "none".into())
    ));
    out.push_str(&format!("# top_calls {}\n", s.top_calls));
    for (k, n, ms) in &s.profile {
        out.push_str(&format!("# profile {k} {n} {ms:.1}\n"));
    }
    out.push_str(&format!("# pts_entries {}\n", s.pts_entries));
    for (l, n, mix) in &s.big_nodes {
        out.push_str(&format!("# bignode {l} {n} {mix}\n"));
    }
    out.push_str(&format!(
        "# sabotage {}\n",
        pf.sabotage.clone().unwrap_or_else(|| "none".into())
    ));
    for (i, p) in pf.module_paths.iter().enumerate() {
        out.push_str(&format!("M {i} {p}\n"));
    }
    let mut ts: Vec<&TemplateFacts> = pf.templates.iter().collect();
    ts.sort_by(|a, b| a.display.cmp(&b.display).then(a.id.cmp(&b.id)));
    for t in &ts {
        for l in template_lines(t) {
            out.push_str(&l);
            out.push('\n');
        }
    }
    for t in &ts {
        for l in fact_lines(t) {
            out.push_str(&l);
            out.push('\n');
        }
    }
    for st in &pf.sites {
        let v = match &st.verdict {
            Verdict::T0Mono => "T0".to_string(),
            Verdict::T0Set => "T0set".to_string(),
            Verdict::E0 => "E0".to_string(),
            Verdict::No(r) => format!("no:{r}"),
        };
        out.push_str(&format!(
            "S {} {} .{} call={} fixed={} recv={{{}}} top={} targets={{{}}} verdict={}\n",
            st.module,
            st.byte_offset,
            crate::model::esc_key(&st.key),
            b(st.is_call),
            b(st.fixed_shape),
            st.receivers.join(","),
            b(st.top),
            st.targets.join(","),
            v
        ));
    }
    out
}

pub fn relations_text(pf: &ProgramFacts) -> Option<String> {
    let rel = pf.relations.as_ref()?;
    let mut out = String::with_capacity(rel.len() * 24);
    out.push_str("# wpfacts relations v1\n");
    for (i, n) in pf.tok_names.iter().enumerate() {
        out.push_str(&format!("tokname {i} {n}\n"));
    }
    for l in rel {
        out.push_str(l);
        out.push('\n');
    }
    Some(out)
}

pub fn module_digests(pf: &ProgramFacts) -> Vec<u64> {
    let n = pf.module_paths.len();
    // The templates each module can observe: its own, and those in its
    // sites' receiver sets (deduplicated before any text is built).
    let mut seen: Vec<std::collections::BTreeSet<usize>> = vec![Default::default(); n];
    let by_display: std::collections::HashMap<&str, usize> = pf
        .templates
        .iter()
        .enumerate()
        .map(|(i, t)| (t.display.as_str(), i))
        .collect();
    for (i, t) in pf.templates.iter().enumerate() {
        if let Some(m) = t.module {
            if (m as usize) < n {
                seen[m as usize].insert(i);
            }
        }
    }
    for s in &pf.sites {
        let m = s.module as usize;
        if m >= n {
            continue;
        }
        for r in &s.receivers {
            if let Some(&i) = by_display.get(r.as_str()) {
                seen[m].insert(i);
            }
        }
    }
    // One hash per template, combined in id order per module.
    let per_tmpl: Vec<u64> = pf
        .templates
        .iter()
        .map(|t| {
            let mut h = FNV_OFFSET;
            for l in template_lines(t) {
                fnv(&mut h, &l);
            }
            h
        })
        .collect();
    let global = pf.interp_guard.clone().unwrap_or_default();
    let sab = pf.sabotage.clone().unwrap_or_default();
    seen.into_iter()
        .map(|ids| {
            let mut h = FNV_OFFSET;
            fnv(&mut h, "wpfacts-v1");
            fnv(&mut h, &global);
            fnv(&mut h, &format!("{:?}", pf.dynamic_code_reach));
            fnv(&mut h, &sab);
            for i in ids {
                fnv(&mut h, &format!("{:016x}", per_tmpl[i]));
            }
            h
        })
        .collect()
}

/// `.perry_facts` payload: the claims about the class templates a module
/// defines. Little-endian; parsed by perry-runtime `contracts::facts`.
///
/// ```text
/// "PFCT" u32 version=2 u32 count
///   u32 class_id  u32 flags(bit0 prefix_stable, bit1 fixed_layout)
///   u16 nanc  u32 ancestor_class_id * nanc
///   u16 name_len name  u16 nkeys
///     u16 key_len key  u8 flags(bit0 readonly, bit1 definite, bit2 birth)
///     u16 kind_mask (0xffff = no claim; prim bits; bit7 object)
/// ```
pub fn contract_section(pf: &ProgramFacts, module: u32) -> Vec<u8> {
    let mut recs: Vec<Vec<u8>> = Vec::new();
    for t in &pf.templates {
        if t.module != Some(module) {
            continue;
        }
        let Some(cid) = t.class_id else {
            continue;
        };
        if cid == 0 {
            continue;
        }
        let claims_any = t.prefix_stable
            || t.fields
                .iter()
                .any(|f| f.readonly || f.kind.mask() != 0xffff);
        if !claims_any {
            continue;
        }
        let mut r = Vec::new();
        r.extend_from_slice(&cid.to_le_bytes());
        let flags = (t.prefix_stable as u32) | ((t.fixed_layout as u32) << 1);
        r.extend_from_slice(&flags.to_le_bytes());
        r.extend_from_slice(&(t.ancestors.len() as u16).to_le_bytes());
        for a in &t.ancestors {
            r.extend_from_slice(&a.to_le_bytes());
        }
        let name = t.display.as_bytes();
        r.extend_from_slice(&(name.len().min(u16::MAX as usize) as u16).to_le_bytes());
        r.extend_from_slice(&name[..name.len().min(u16::MAX as usize)]);
        let keys: Vec<_> = t.fields.iter().take(u16::MAX as usize).collect();
        r.extend_from_slice(&(keys.len() as u16).to_le_bytes());
        for f in keys {
            let k = f.key.as_bytes();
            r.extend_from_slice(&(k.len().min(u16::MAX as usize) as u16).to_le_bytes());
            r.extend_from_slice(&k[..k.len().min(u16::MAX as usize)]);
            let birth = t.birth_keys.iter().any(|b| *b == f.key);
            let fl = (f.readonly as u8) | ((f.definite as u8) << 1) | ((birth as u8) << 2);
            r.push(fl);
            r.extend_from_slice(&f.kind.mask().to_le_bytes());
        }
        recs.push(r);
    }
    if recs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"PFCT");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(recs.len() as u32).to_le_bytes());
    for r in recs {
        out.extend_from_slice(&r);
    }
    // Keep each module's blob 8-byte aligned so blobs concatenated by the
    // linker stay parseable back to back.
    while out.len() % 8 != 0 {
        out.push(0);
    }
    out
}

fn matches_name(display: &str, name: &str) -> bool {
    display == name
        || display
            .rsplit_once(':')
            .is_some_and(|(_, tail)| tail == name)
}

/// Plant one wrong fact (negative control, design 6.5).
pub fn apply_sabotage(ts: &mut [TemplateFacts], spec: &str) {
    let Some((what, target)) = spec.split_once(':') else {
        return;
    };
    let (target, value) = match target.split_once('=') {
        Some((t, v)) => (t, Some(v)),
        None => (target, None),
    };
    let (tname, key) = match target.rsplit_once('.') {
        Some((t, k)) if what != "prefix_stable" && what != "fixed_layout" => (t, Some(k)),
        _ => (target, None),
    };
    for t in ts.iter_mut().filter(|t| matches_name(&t.display, tname)) {
        match (what, key) {
            ("prefix_stable", _) => t.prefix_stable = true,
            ("fixed_layout", _) => {
                t.prefix_stable = true;
                t.fixed_layout = true;
            }
            ("readonly", Some(k)) => {
                for f in t.fields.iter_mut().filter(|f| f.key == k) {
                    f.readonly = true;
                }
            }
            ("definite", Some(k)) => {
                for f in t.fields.iter_mut().filter(|f| f.key == k) {
                    f.definite = true;
                }
            }
            ("kind", Some(k)) => {
                let p = match value.unwrap_or("num") {
                    "str" => crate::model::prim::STR,
                    "bool" => crate::model::prim::BOOL,
                    "obj" => 0,
                    _ => crate::model::prim::NUM,
                };
                for f in t.fields.iter_mut().filter(|f| f.key == k) {
                    f.kind.top = false;
                    f.kind.prims = p;
                    f.kind.funcs = 0;
                    f.kind.builtin_objs = 0;
                    if p != 0 {
                        f.kind.objs.clear();
                    } else if f.kind.objs.is_empty() {
                        f.kind.objs.push("sabotage".into());
                    }
                }
            }
            _ => continue,
        }
        t.sabotaged = true;
    }
}
