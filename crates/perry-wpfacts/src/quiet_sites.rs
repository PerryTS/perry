//! Quiet sites: a read, a store or a value that runs no program code
//! (`ModuleBindings::quiet_reads`, `quiet_elements`, `primitive_reads`,
//! `primitive_elements`).
//!
//! A loop "without calls" is not a loop that runs only its own function's
//! code: a property store can run a setter or a Proxy trap, a property
//! read a getter, an operator `valueOf` / `toString` / `@@toPrimitive` of
//! an operand, a computed key its `toString`. `perry-codegen` keeps
//! `a.length` over such a loop (`stmt::loops`, `classify_for_length_hoist`)
//! on the premise that nothing but the loop's own stores can change it, so
//! each such site of the loop must be known to run no program code. That
//! is what the solve knows and the text of the loop does not; it is
//! answered here, per site:
//!
//! - **a quiet read** (`quiet_reads`, a named `PropertyGet` by its byte
//!   offset and key): at every copy of the site its verdict (`derive::Verdict`) is
//!   `OwnLength`, `NativeMember`, `T0Mono` or `T0Set`: an own data slot of
//!   every receiver, or the realm's intrinsic member. No getter, no Proxy
//!   (a Proxy is ⊤, which has no such verdict);
//! - **a quiet element site** (`quiet_elements`, an `IndexGet`, `IndexSet`,
//!   `IndexUpdate` or `PutValueSet` by its element offset): at every copy,
//!   and every summary instance's copy of it, the receiver holds only plain
//!   program Arrays (no ⊤, no marker, no escaped or foreign Array, none
//!   with an accessor or a named key defined on it) or only Strings, and the
//!   key holds only Numbers (ToPropertyKey runs nothing); no unit solved a
//!   native-prototype write and no accessor sits on `Object.prototype`, so
//!   no index accessor is on the Arrays' chain. These are the conditions
//!   `field_read_calls` (5a to 5d) puts on its element read;
//! - **a primitive value** (`primitive_reads` by a named read's byte
//!   offset, `primitive_elements` by an element read's offset): at every
//!   copy the value node holds a primitive and no object at all. An
//!   operator whose operands are such values converts nothing
//!   (ToPrimitive of a primitive is the identity). A local's own kinds are
//!   in its `BindingFacts` (`prims`, `other`, `class_ids`).
//!
//! - **a String's code unit** (`PERRY_FACTS_QUIET_STRINGS`): the value of
//!   an element read is primitive when the site is a quiet element site
//!   whose receivers (and every summary instance's copy) hold Strings and
//!   nothing else, and the value node holds no object: in range the read is
//!   the String's own code unit, out of range it goes up `String.prototype`
//!   and `Object.prototype`, on which no unit solved a write (the quiet
//!   element site's own condition), so it is `undefined`. The solve leaves
//!   the value of such a read without a kind (no object, and no primitive
//!   either), which is all this rule adds; nothing it relies on is derived
//!   from syntax or a declared type. Dump line `verdict=quiet by=string`;
//!
//! No fact for a site means the site may run code: an unknown receiver, a
//! computed key that may be an object, a value that may be anything. There
//! is no fact at all in an aborted or per-unit solve or when interpreted
//! code can run.
//!
//! `PERRY_FACTS_QUIET_SITES=0` exports nothing (and codegen asks nothing:
//! the walker of before); `PERRY_FACTS_QUIET_STRINGS=0` drops the String
//! rule alone. The negative control `PERRY_FACTS_SABOTAGE=quiet` claims
//! every site quiet and every value primitive; `quiet-strings` claims the
//! String rule for every quiet element read. The dump
//! (`PERRY_FACTS_DUMP`) prints one `# quiet` line per site:
//! `kind=read|element|value|elemvalue module= offset= verdict=quiet`, or
//! `refused=<why>`.

use crate::bindings::{owner_metadata_written, ModuleBindings};
use crate::derive::{SiteFacts, Verdict};
use crate::gen::Gen;
use crate::model::{prim, Model, NodeId, TokKind, TOP};
use std::collections::BTreeMap;

/// `PERRY_FACTS_QUIET_SITES=0` turns the fact off.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_QUIET_SITES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_FACTS_QUIET_STRINGS=0` turns the String rule off (module docs).
pub fn strings_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_QUIET_STRINGS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Node `n` holds Strings and nothing else.
fn strings_only(m: &Model, n: NodeId) -> bool {
    let node = &m.nodes[n as usize];
    node.pts.is_empty() && node.prim == prim::STR
}

/// Why node `n` may hold something that is no plain program Array
/// (`None`: it holds such Arrays and nothing else, or only Strings).
fn not_plain_arrays(m: &Model, n: NodeId) -> Option<&'static str> {
    let node = &m.nodes[n as usize];
    // A String's own index is a data property, and past its end the read
    // goes up the chain as an Array's hole does.
    if node.prim == prim::STR && node.pts.is_empty() {
        return None;
    }
    if node.prim != 0 {
        return Some("receiver:primitive");
    }
    if node.pts.is_empty() {
        return Some("receiver:empty");
    }
    if node.pts.contains_tok(TOP) {
        return Some("receiver:top");
    }
    for t in node.pts.to_sorted() {
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            return Some("receiver:not-array");
        };
        if !m.is_arr(t) {
            return Some("receiver:not-array");
        }
        if m.is_marker(t) {
            return Some("receiver:marker");
        }
        if m.is_escaped(t) || m.foreign.contains_key(&tm) {
            return Some("receiver:escaped");
        }
        if owner_metadata_written(m, tm) || m.accessor_keys.get(&t).is_some_and(|k| !k.is_empty()) {
            return Some("receiver:defined-key");
        }
    }
    None
}

/// Node `n` holds at least one primitive and no object.
fn primitive(m: &Model, n: NodeId) -> bool {
    let node = &m.nodes[n as usize];
    node.pts.is_empty() && node.prim != 0
}

/// Node `n` holds Numbers and nothing else.
fn number(m: &Model, n: NodeId) -> bool {
    let node = &m.nodes[n as usize];
    node.pts.is_empty() && node.prim == prim::NUM
}

/// Fill the quiet-site lists of every module (module docs). `interpreted`:
/// interpreted code can run (`ProgramFacts::interp_guard`).
pub(crate) fn collect(
    g: &Gen<'_>,
    sites: &[SiteFacts],
    bindings: &mut [ModuleBindings],
    interpreted: bool,
    sabotage: Option<&str>,
    mut probe: Option<&mut Vec<String>>,
) {
    let string_sabotage = sabotage == Some("quiet-strings");
    let sabotage = sabotage == Some("quiet");
    let strings = strings_enabled() || string_sabotage;
    let m = &g.m;
    if !enabled() || m.aborted || m.unit_mode || interpreted {
        return;
    }
    // A summarized body's node has one copy per instance (`Inst::fresh`):
    // the fact must hold for each of them too.
    let mut copies: std::collections::HashMap<NodeId, Vec<NodeId>> = Default::default();
    for inst in 0..m.sum.insts.len() as u32 {
        m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    let with_copies =
        |n: NodeId| std::iter::once(n).chain(copies.get(&n).into_iter().flatten().copied());
    // No index accessor on the chain of any Array.
    let chain = if !m.native_writes.is_empty() {
        Some("chain:native-write")
    } else if m.objproto_accessor {
        Some("chain:object-prototype-accessor")
    } else {
        None
    };
    let join = |slot: &mut Option<&'static str>, why: Option<&'static str>| {
        if slot.is_none() {
            *slot = why;
        }
    };
    // (module, offset, key) -> the first reason a copy is not quiet. A
    // member chain `a.b.c` starts each of its reads at one offset: the key
    // tells them apart.
    type Sites<K> = BTreeMap<(u32, u32, K), Option<&'static str>>;
    let mut reads: Sites<String> = BTreeMap::new();
    for s in sites.iter().filter(|s| !s.is_call && s.byte_offset != 0) {
        let why = match s.verdict {
            Verdict::OwnLength | Verdict::NativeMember | Verdict::T0Mono | Verdict::T0Set => None,
            // No value reaches the base: the read is never evaluated and runs nothing.
            Verdict::Dead => None,
            // Fact-unit receivers with no accessor, no escape and the chain proven
            // absent: OrdinaryGet reads a data slot or undefined, it runs no code.
            Verdict::OwnDataOrAbsent => None,
            // A namespace [[Get]] is GetBindingValue: no getter, no trap, unknown code
            // cannot change it, and a TDZ throw runs no program code.
            Verdict::NsExport => None,
            // A value-call verdict, never a named read's: the intrinsic's [[Call]] may
            // run program code (callbacks, ToPrimitive).
            Verdict::IntrinsicCall => Some("verdict:other"),
            Verdict::E0 => Some("verdict:guarded"),
            Verdict::No(_) => Some("verdict:none"),
        };
        join(
            reads
                .entry((s.module, s.byte_offset, s.key.clone()))
                .or_insert(None),
            why,
        );
    }
    let mut elements: Sites<()> = BTreeMap::new();
    // Element sites whose every receiver copy holds Strings alone.
    let mut string_elements: BTreeMap<(u32, u32), bool> = BTreeMap::new();
    for e in g.element_sites.iter().filter(|e| e.byte_offset != 0) {
        let mut why = chain;
        for n in with_copies(e.recv) {
            join(&mut why, not_plain_arrays(m, n));
        }
        if strings {
            let all = string_sabotage || with_copies(e.recv).all(|n| strings_only(m, n));
            *string_elements
                .entry((e.module, e.byte_offset))
                .or_insert(true) &= all;
        }
        for n in with_copies(e.idx) {
            join(&mut why, (!number(m, n)).then_some("key:not-number"));
        }
        join(
            elements
                .entry((e.module, e.byte_offset, ()))
                .or_insert(None),
            why,
        );
    }
    let mut values: Sites<String> = BTreeMap::new();
    for &(mi, offset, key, dst) in &g.load_sites {
        let all = with_copies(dst).all(|n| primitive(m, n));
        join(
            values
                .entry((mi, offset, m.atoms[key as usize].to_string()))
                .or_insert(None),
            (!all).then_some("value:may-be-object"),
        );
    }
    let mut elem_values: Sites<()> = BTreeMap::new();
    let mut string_elem_values: std::collections::BTreeSet<(u32, u32)> = Default::default();
    for &(mi, offset, n) in &g.element_values {
        let all = with_copies(n).all(|n| primitive(m, n));
        // A code unit or `undefined`: a quiet element site over Strings
        // alone, whose value node holds no object.
        let by_string = !all
            && string_elements.get(&(mi, offset)) == Some(&true)
            && (string_sabotage
                || elements
                    .get(&(mi, offset, ()))
                    .is_some_and(|why| why.is_none())
                    && with_copies(n).all(|n| m.nodes[n as usize].pts.is_empty()));
        if by_string {
            string_elem_values.insert((mi, offset));
        }
        join(
            elem_values.entry((mi, offset, ())).or_insert(None),
            (!all && !by_string).then_some("value:may-be-object"),
        );
    }
    let mut line = |kind: &str,
                    mi: u32,
                    offset: u32,
                    key: &str,
                    why: Option<&'static str>,
                    by_string: bool| {
        if let Some(p) = probe.as_deref_mut() {
            let key = if key.is_empty() {
                String::new()
            } else {
                format!(" key={key}")
            };
            let by = if by_string { " by=string" } else { "" };
            p.push(match why {
                None => {
                    format!(
                        "# quiet kind={kind} module={mi} offset={offset}{key} verdict=quiet{by}"
                    )
                }
                Some(why) => {
                    format!("# quiet kind={kind} module={mi} offset={offset}{key} refused={why}")
                }
            });
        }
    };
    // The maps are ordered by (module, offset, key): each list comes out
    // sorted.
    for (kind, map) in [("read", reads), ("value", values)] {
        for ((mi, offset, key), why) in map {
            let why = why.filter(|_| !sabotage);
            line(kind, mi, offset, &key, why, false);
            if let (None, Some(mb)) = (why, bindings.get_mut(mi as usize)) {
                match kind {
                    "read" => mb.quiet_reads.push((offset, key)),
                    _ => mb.primitive_reads.push((offset, key)),
                }
            }
        }
    }
    for (kind, map) in [("element", elements), ("elemvalue", elem_values)] {
        for ((mi, offset, ()), why) in map {
            let by_string = kind == "elemvalue" && string_elem_values.contains(&(mi, offset));
            let why = why.filter(|_| !sabotage);
            line(kind, mi, offset, "", why, by_string);
            if let (None, Some(mb)) = (why, bindings.get_mut(mi as usize)) {
                match kind {
                    "element" => mb.quiet_elements.push(offset),
                    _ => mb.primitive_elements.push(offset),
                }
            }
        }
    }
    if let Some(p) = probe.as_deref_mut() {
        p.push(format!(
            "# quiet strings elements={} elemvalues={}",
            string_elements.values().filter(|all| **all).count(),
            string_elem_values.len()
        ));
    }
}
