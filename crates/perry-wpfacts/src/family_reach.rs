//! `family_reach`: per runtime family, whether the program may ever create
//! or observe a value of it (DESIGN-family-reach, as amended by its review
//! sections 9 to 12). `never` is the only actionable value. No link, codegen
//! or runtime decision reads the fact yet: it is published on
//! [`crate::ProgramFacts`] and dumped.
//!
//! A family is one install-hook group of the runtime; the list and the
//! membership of realm entries are one table,
//! `perry_runtime_manifest::families`. A family is live as soon as one rule
//! fires, and the first rule that fires is the dump's reason:
//!
//! - **L0** the solve is incomplete (aborted, widened, per unit): every
//!   family.
//! - **L1** an unknown world: interpreted code may run, a call of an unknown
//!   function value was solved, some node holds ⊤ (every undeclared built-in
//!   result, native module, FFI value and unknown global is a ⊤ origin), the
//!   global object is read under an unknown key, or a generic built-in
//!   function value was minted at a site this file does not classify: every
//!   family. A program with unknown values (three.js, tsc) has every family
//!   live; the fact says `never` only in a closed program.
//! - **L2** the solve resolved a load, store, call or construct to a realm
//!   entry of the family (member granularity: `process.argv`, not
//!   `process`), ran a declared native, minted a native function value, or
//!   holds a built-in object or holder of the family as a token.
//! - **L3** the program text names the family ([`text`]): a syntactic
//!   predicate for the same fact. It only ever adds `live`, and it does not
//!   read which bodies can run except for its computed-key part.
//! - **L4** closure: a live family requires it, or an observed entry
//!   declares the birth of one of its entries.
//!
//! The rules run inside the one solve: L2 is recorded by one-line hooks in
//! the functions that resolve a built-in receiver and key
//! (`builtins/loads.rs`, `builtins/methods.rs`, `builtins/effects.rs`,
//! `builtins.rs`, `builtins/constructors.rs`) and read from the model's own
//! token table after the fixpoint; L0, L1, L3 and L4 are read in
//! [`derive`]. The activation pass and the final pass run the same rules;
//! the published set is the final pass's.

use crate::model::{prim, BuiltinNs, Key, Model, Native, TmplKind, Tok, TokKind, TOP};
use crate::pts::FxHashMap;
use perry_runtime_manifest::families::{self, FamilySet, RUNTIME_FAMILIES};

mod nodes;
mod text;

/// `PERRY_FACTS_FAMILY_REACH=0` (or `off`, `false`): the fact is not
/// computed and not dumped; a reader treats every family as live.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_FAMILY_REACH").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// One family's verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyVerdict {
    /// The family's id (`perry_runtime_manifest::families`).
    pub family: &'static str,
    pub live: bool,
    /// The first rule that fired, `<rule>:<what>`; `none` for `never`.
    pub reason: String,
}

/// The published fact: one verdict per family, in table order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FamilyReach {
    pub families: Vec<FamilyVerdict>,
}

impl FamilyReach {
    /// Whether family `id` may be created or observed. An unknown id is
    /// live: only a `never` this fact states is a claim.
    pub fn live(&self, id: &str) -> bool {
        self.families
            .iter()
            .find(|f| f.family == id)
            .is_none_or(|f| f.live)
    }

    /// The families the fact says are never created or observed, as a set
    /// in table order.
    pub fn never(&self) -> FamilySet {
        self.families
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.live)
            .fold(0, |set, (i, _)| set | (1 << i))
    }

    /// The dump's lines: `# family_reach <F> live|never reason=<rule>:<what>`.
    pub fn dump_lines(&self) -> Vec<String> {
        self.families
            .iter()
            .map(|f| {
                format!(
                    "# family_reach {} {} reason={}",
                    f.family,
                    if f.live { "live" } else { "never" },
                    f.reason.replace(['\n', '\r'], " ")
                )
            })
            .collect()
    }
}

/// What the hooks recorded during one solve (`Model::family_reach`).
#[derive(Default)]
pub(crate) struct State {
    /// The fact is computed (the switch is on and the solve is whole-program).
    pub(crate) on: bool,
    live: FamilySet,
    why: Vec<Option<String>>,
    /// An L1 event a hook saw.
    all: Option<String>,
}

impl State {
    fn mark(&mut self, set: FamilySet, why: impl FnOnce() -> String) {
        let new = set & !self.live;
        if new == 0 {
            return;
        }
        if self.why.len() < RUNTIME_FAMILIES.len() {
            self.why.resize(RUNTIME_FAMILIES.len(), None);
        }
        let why = why();
        for (i, slot) in self.why.iter_mut().enumerate() {
            if new & (1 << i) != 0 {
                *slot = Some(why.clone());
            }
        }
        self.live |= new;
    }
}

/// A member of a holder that is of a family, or declares the birth of an
/// entry of one: its key, the families and the entry's name.
type Member = (&'static str, FamilySet, &'static str);

/// What one realm entry observes: its own families and the families of the
/// entries it declares born.
#[derive(Clone, Copy, Default)]
struct Entry {
    own: FamilySet,
    births: FamilySet,
}

/// The realm table, grouped for the rules. Built once per process from
/// `native_effects::realm()` and the family table; it holds no state of a
/// solve.
pub(crate) struct Index {
    entries: FxHashMap<&'static str, Entry>,
    /// Holder (any entry with members) -> its members of a family.
    members: FxHashMap<&'static str, Vec<Member>>,
    /// A holder of a family (a root or any entry below it) -> what observing
    /// it observes: its families and its declared births.
    holders: FxHashMap<&'static str, FamilySet>,
    /// Root holder -> everything reachable below it: the families of its
    /// entries and of their declared births (the text part's closure).
    roots: FxHashMap<&'static str, FamilySet>,
    /// Key -> the entries of a family under that key on a core holder.
    core_keys: FxHashMap<&'static str, (FamilySet, &'static str)>,
    /// Key -> the direct members of a family's namespace under that key
    /// (`floor` of `Math`: lowering can drop the namespace's name).
    namespace_keys: FxHashMap<&'static str, (FamilySet, &'static str)>,
    /// Every member of a family (or producer of one) on a core prototype.
    computed: Vec<(FamilySet, &'static str)>,
    string_proto: Vec<Member>,
    number_proto: Vec<Member>,
    bigint_proto: Vec<Member>,
    array_proto: Vec<Member>,
    function_proto: Vec<Member>,
    object_proto: Vec<Member>,
}

/// The families of realm entry `name`; a name the family table does not
/// classify is every family (the realm test keeps that set empty).
fn families_of(name: &str) -> FamilySet {
    families::entry_families(name).unwrap_or_else(families::all_families)
}

/// `Holder.key` -> (`Holder`, `key`); `None` for a name without a member
/// key (a root, a symbol-keyed member).
fn split_member(name: &'static str) -> Option<(&'static str, &'static str)> {
    let name = name
        .strip_suffix(".<get>")
        .or_else(|| name.strip_suffix(".<set>"))
        .unwrap_or(name);
    if name.ends_with(']') || name.ends_with('%') {
        return None;
    }
    let (holder, key) = name.rsplit_once('.')?;
    // `node:path.posix` is a module, not member `posix` of `node:path`.
    (families::entry_module(name).is_none_or(|module| {
        name.strip_prefix("node:")
            .is_none_or(|rest| rest.len() > module.len())
    }))
    .then_some((holder, key))
}

/// Entries that hold every global: naming one names no family by itself.
fn is_global_holder(name: &str) -> bool {
    matches!(name, "%Global%" | "globalThis" | "global")
}

pub(crate) fn index() -> &'static Index {
    static INDEX: std::sync::OnceLock<Index> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let realm = crate::native_effects::realm();
        let mut ix = Index {
            entries: Default::default(),
            members: Default::default(),
            holders: Default::default(),
            roots: Default::default(),
            core_keys: Default::default(),
            namespace_keys: Default::default(),
            computed: Vec::new(),
            string_proto: Vec::new(),
            number_proto: Vec::new(),
            bigint_proto: Vec::new(),
            array_proto: Vec::new(),
            function_proto: Vec::new(),
            object_proto: Vec::new(),
        };
        for e in realm {
            let own = families_of(e.name);
            let births = if is_global_holder(e.name) {
                0
            } else {
                e.births
                    .iter()
                    .filter(|b| !is_global_holder(b))
                    .fold(0, |set, b| set | families_of(b))
            };
            ix.entries.insert(e.name, Entry { own, births });
        }
        // The global namespace objects (`Math`, `JSON`, `Intl`): lowering
        // can keep a read of a member of one as a read off the global scope.
        let namespaces: crate::pts::FxHashSet<&'static str> = realm
            .iter()
            .filter(|e| {
                matches!(e.kind, perry_hir::realm_manifest::RealmKind::Namespace)
                    && families::entry_root(e.name) == e.name
                    && families::entry_module(e.name).is_none()
            })
            .map(|e| e.name)
            .collect();
        for e in realm {
            let Entry { own, births } = ix.entries[e.name];
            let root = families::entry_root(e.name);
            let root_own = families_of(root);
            *ix.roots.entry(root).or_default() |= own | births;
            if root_own != 0 || families::entry_module(e.name).is_some() {
                // A holder of a family: observing any entry of it observes
                // the family and what the entry declares born.
                if own | births != 0 {
                    ix.holders.insert(e.name, own | births);
                }
            }
            let Some((holder, key)) = split_member(e.name) else {
                continue;
            };
            if own | births == 0 {
                continue;
            }
            ix.members
                .entry(holder)
                .or_default()
                .push((key, own | births, e.name));
            if root_own == 0 && families::entry_module(e.name).is_none() {
                // A member of a family (or a producer of one) on a core
                // holder.
                let slot = ix.core_keys.entry(key).or_insert((0, e.name));
                slot.0 |= own | births;
                // A prototype's values are present without being named.
                if holder.contains(".prototype") || holder.starts_with('%') {
                    ix.computed.push((own | births, e.name));
                }
            } else if holder == root && namespaces.contains(root) {
                let slot = ix.namespace_keys.entry(key).or_insert((0, e.name));
                slot.0 |= own | births;
            }
        }
        let of = |ix: &Index, holder: &str| ix.members.get(holder).cloned().unwrap_or_default();
        ix.string_proto = of(&ix, "String.prototype");
        ix.number_proto = of(&ix, "Number.prototype");
        ix.bigint_proto = of(&ix, "BigInt.prototype");
        ix.array_proto = of(&ix, "Array.prototype");
        ix.function_proto = of(&ix, "Function.prototype");
        ix.object_proto = of(&ix, "Object.prototype");
        ix
    })
}

impl Index {
    /// What a mention of holder `root` as a value observes: every entry
    /// below it and what they declare born.
    pub(crate) fn root(&self, root: &str) -> FamilySet {
        self.roots.get(root).copied().unwrap_or(0)
    }

    pub(crate) fn entry(&self, name: &str) -> Option<FamilySet> {
        self.entries.get(name).map(|e| e.own | e.births)
    }
}

/// The name of family set `set`'s first family (for a reason).
fn first_name(set: FamilySet) -> &'static str {
    RUNTIME_FAMILIES
        .get(set.trailing_zeros() as usize)
        .map_or("?", |f| f.id)
}

// ------------------------------------------------------------- L2 hooks

/// The member of `members` key `key` may name.
fn observe_members(m: &mut Model, members: &'static [Member], key: Key) {
    if members.is_empty() {
        return;
    }
    for &(k, set, entry) in members {
        if set & !m.family_reach.live == 0 {
            continue;
        }
        let named = match key {
            Key::Named(a) => m.atoms[a as usize] == k,
            Key::Any(_) => m.key_may_name_str(key, k),
        };
        if named {
            m.family_reach.mark(set, || match key {
                Key::Named(_) => format!("L2:{entry}"),
                Key::Any(_) => format!("L2:{entry} under a computed key"),
            });
        }
    }
}

fn observe_holder(m: &mut Model, holder: &str, key: Key) {
    let ix = index();
    if let Some(&set) = ix.holders.get(holder) {
        m.family_reach.mark(set, || format!("L2:{holder}"));
    }
    if let Some(members) = ix.members.get(holder) {
        observe_members(m, members, key);
    }
}

/// The realm holder whose members a built-in object made by a declaration
/// or a hand-written model reads (`TmplKind::BObj::what`).
fn bobj_holder(what: &str) -> std::borrow::Cow<'static, str> {
    match what {
        "TypedArray" => "%TypedArray%.prototype".into(),
        w if w.ends_with(".prototype") => w.to_owned().into(),
        w => format!("{w}.prototype").into(),
    }
}

fn observe_tok(m: &mut Model, t: Tok, key: Key) {
    let ix = index();
    let holder: std::borrow::Cow<'static, str> = match &m.toks[t as usize] {
        TokKind::Builtin(BuiltinNs::Other | BuiltinNs::GlobalThis) => {
            return observe_global(m, key)
        }
        TokKind::Builtin(_) => match m.native_shape(t) {
            Some(shape) => shape.name.into(),
            None => return,
        },
        TokKind::NativeProto(n) => format!("{n}.prototype").into(),
        TokKind::Obj(tm, _) => match m.tmpls[*tm as usize].kind {
            TmplKind::BObj { what, .. } => bobj_holder(what),
            TmplKind::Arr { .. } => return observe_members(m, &ix.array_proto, key),
            _ => return,
        },
        TokKind::Func(..) | TokKind::Bound(_) | TokKind::BuiltinFn(_) => {
            return observe_members(m, &ix.function_proto, key)
        }
        TokKind::ObjectProto => return observe_members(m, &ix.object_proto, key),
        _ => return,
    };
    observe_holder(m, &holder, key);
}

/// A read, write or call of global `key` off the global object.
fn observe_global(m: &mut Model, key: Key) {
    let Key::Named(a) = key else {
        if m.family_reach.all.is_none() {
            m.family_reach.all =
                Some("L1:the global object is read or written under an unknown key".into());
        }
        return;
    };
    let ix = index();
    let name = &m.atoms[a as usize];
    // A global of a family: the family and what its declaration says is
    // born. A core global names no family by itself (its members are
    // observed one by one).
    if let Some(&set) = ix.holders.get(name.as_str()) {
        let name = name.clone();
        m.family_reach.mark(set, || format!("L2:global {name}"));
    }
}

/// C1: a load, store or call resolved on receiver token `t` under `key`.
#[inline]
pub(crate) fn tok(m: &mut Model, t: Tok, key: Key) {
    if m.family_reach.on {
        observe_tok(m, t, key);
    }
}

/// C1: a load through the chain of `of` at prototype `q`.
#[inline]
pub(crate) fn chain(m: &mut Model, of: Tok, q: Tok, key: Key) {
    if m.family_reach.on {
        observe_tok(m, of, key);
        observe_tok(m, q, key);
    }
}

/// C1: a load or call on a primitive of kinds `mask` under `key`.
#[inline]
pub(crate) fn primitive(m: &mut Model, mask: u16, key: Key) {
    if !m.family_reach.on {
        return;
    }
    let ix = index();
    if mask & prim::STR != 0 {
        observe_members(m, &ix.string_proto, key);
    }
    if mask & prim::NUM != 0 {
        observe_members(m, &ix.number_proto, key);
    }
    if mask & prim::BIGINT != 0 {
        observe_members(m, &ix.bigint_proto, key);
    }
}

/// C1: a call, construct or member read on built-in namespace `ns` (member
/// `name`; `None`: the namespace itself is called or constructed).
#[inline]
pub(crate) fn namespace(m: &mut Model, ns: BuiltinNs, name: Option<&str>) {
    if !m.family_reach.on {
        return;
    }
    // A namespace no token names was reached from a lowered node, which the
    // text part classifies.
    let Some(&t) = m.tok_index.get(&TokKind::Builtin(ns)) else {
        return;
    };
    let key = match name {
        Some(n) => match m.atom_index.get(n) {
            Some(&a) => Key::Named(a),
            None => return observe_named(m, t, n),
        },
        // The holder itself: any key of it names what the call observes.
        None => return observe_named(m, t, ""),
    };
    observe_tok(m, t, key);
}

/// `observe_tok` for a member name that is no atom of the model.
fn observe_named(m: &mut Model, t: Tok, name: &str) {
    if matches!(
        m.toks[t as usize],
        TokKind::Builtin(BuiltinNs::Other | BuiltinNs::GlobalThis)
    ) {
        if let Some(&set) = index().holders.get(name) {
            m.family_reach.mark(set, || format!("L2:global {name}"));
        }
        return;
    }
    let Some(holder) = m.native_shape(t).map(|s| s.name) else {
        return;
    };
    let ix = index();
    if let Some(&set) = ix.holders.get(holder) {
        m.family_reach.mark(set, || format!("L2:{holder}"));
    }
    for &(k, set, entry) in ix.members.get(holder).map_or(&[][..], |v| v) {
        if k == name {
            m.family_reach.mark(set, || format!("L2:{entry}"));
        }
    }
}

/// C1: member `a` read directly off the intrinsic prototype of the
/// constructor global `global` names (`%TypedArray%` for the abstract one).
#[inline]
pub(crate) fn prototype(m: &mut Model, global: &str, a: crate::model::Atom) {
    if m.family_reach.on {
        observe_holder(m, &format!("{global}.prototype"), Key::Named(a));
    }
}

/// C1: a named global constructed or called by name (`new TextEncoder()`).
#[inline]
pub(crate) fn global(m: &mut Model, name: &str) {
    if !m.family_reach.on {
        return;
    }
    if let Some(&set) = index().holders.get(name) {
        m.family_reach.mark(set, || format!("L2:global {name}"));
    }
}

/// C2: declared native `entry` runs: its own family and what it declares
/// born.
#[inline]
pub(crate) fn run(m: &mut Model, entry: &'static str) {
    if !m.family_reach.on {
        return;
    }
    let Some(e) = index().entries.get(entry) else {
        return;
    };
    m.family_reach.mark(e.own, || format!("L2:{entry} runs"));
    m.family_reach.mark(e.births, || {
        format!(
            "L4:{entry} declares the birth of a {} entry",
            first_name(e.births & !e.own)
        )
    });
}

/// The sites that mint the generic built-in function value
/// (`TokKind::BuiltinFn(Native::Any)`, a native with no identity and no
/// holder), by the `why` each passes to `builtins::builtin_fn`. Every one
/// of them runs under a hooked load of this file, which has the receiver
/// and the key in hand and counts the mint as every entry of that receiver
/// the key may name. A mint at any other site has no receiver here: it is
/// L1.
const GENERIC_MINTS: &[&str] = &[
    "array computed key",
    "builtin namespace static",
    "builtin object computed key",
    "computed key on a described chain",
    "constructor of an undescribed family",
    "described member",
    "function call/apply/bind/toString",
    "native proto / builtin fn computed key",
    "primitive computed key",
    "primitive member",
    "require.<x>",
    "undescribed member",
];

/// C3: the generic built-in function value is minted at site `why`.
#[inline]
pub(crate) fn generic(m: &mut Model, why: &'static str) {
    if m.family_reach.on && m.family_reach.all.is_none() && !GENERIC_MINTS.contains(&why) {
        m.family_reach.all = Some(format!(
            "L1:a generic built-in function value minted at an unclassified site ({why})"
        ));
    }
}

// ---------------------------------------------------------------- derive

/// The accumulator of [`derive`]: the first reason per family wins.
struct Verdicts {
    live: FamilySet,
    why: Vec<Option<String>>,
}

impl Verdicts {
    fn mark(&mut self, set: FamilySet, why: impl FnOnce() -> String) {
        let new = set & families::all_families() & !self.live;
        if new == 0 {
            return;
        }
        let why = why();
        for (i, slot) in self.why.iter_mut().enumerate() {
            if new & (1 << i) != 0 {
                *slot = Some(why.clone());
            }
        }
        self.live |= new;
    }

    fn all(&mut self, why: impl FnOnce() -> String) {
        self.mark(families::all_families(), why);
    }

    fn full(&self) -> bool {
        self.live == families::all_families()
    }
}

/// L2 from the model's own tables: every token the solve minted is a value
/// the program may hold.
fn observe_tokens(m: &Model, v: &mut Verdicts) {
    let ix = index();
    for (t, kind) in m.toks.iter().enumerate() {
        match kind {
            TokKind::Builtin(BuiltinNs::Other | BuiltinNs::GlobalThis) => {}
            TokKind::Builtin(_) => {
                if let Some(shape) = m.native_shape(t as Tok) {
                    if let Some(&set) = ix.holders.get(shape.name) {
                        v.mark(set, || format!("L2:{} is held as a value", shape.name));
                    }
                }
            }
            TokKind::BuiltinFn(Native::Member { identity, .. }) => {
                if let Some(e) = ix.entries.get(identity) {
                    v.mark(e.own, || format!("L2:{identity} is read"));
                    v.mark(e.births, || {
                        format!(
                            "L4:{identity} declares the birth of a {} entry",
                            first_name(e.births & !e.own)
                        )
                    });
                }
            }
            TokKind::NativeProto(n) => {
                let set = families::entry_families(n).unwrap_or(0);
                v.mark(set, || format!("L2:{n}.prototype is held as a value"));
            }
            _ => {}
        }
    }
    for tm in &m.tmpls {
        if let TmplKind::BObj { what, .. } = tm.kind {
            let holder = bobj_holder(what);
            let set = ix
                .holders
                .get(&*holder)
                .copied()
                .or_else(|| families::entry_families(what))
                .unwrap_or(0);
            v.mark(set, || format!("L2:a {what} is born ({})", tm.display));
        }
    }
}

/// The fact, after the fixpoint. `unit`: the modules are one unit of a
/// larger program (L0). `dynamic_code`: the driver linked the interpreter
/// (an input of the solve, reported as such).
pub(crate) fn derive(g: &crate::gen::Gen, unit: bool, dynamic_code: Option<&str>) -> FamilyReach {
    let m = &g.m;
    let mut v = Verdicts {
        live: 0,
        why: vec![None; RUNTIME_FAMILIES.len()],
    };
    // L0.
    if unit {
        v.all(|| "L0:per-unit solve".into());
    } else if m.aborted {
        v.all(|| "L0:solve aborted".into());
    } else if m.widened {
        v.all(|| "L0:solve widened".into());
    }
    // `dyn_eval` is the driver's decision, an input of the solve.
    if let Some(linked) = dynamic_code {
        v.mark(families::family_bit("dyn_eval"), || {
            format!("input:{linked}")
        });
    }
    // L1.
    if !v.full() {
        if m.interp_runs {
            v.all(|| "L1:interpreted code may run".into());
        } else if let Some(why) = &m.dyn_eval {
            v.all(|| format!("L1:dynamic code ({why})"));
        } else if m.top_calls > 0 {
            v.all(|| format!("L1:{} calls of unknown function values", m.top_calls));
        } else if let Some(why) = &m.family_reach.all {
            v.all(|| why.clone());
        } else if let Some(n) = m.nodes.iter().position(|n| n.pts.contains_tok(TOP)) {
            v.all(|| {
                let origin = if m.explain {
                    m.top_origin(n as u32)
                } else {
                    format!("at {}", m.labels.get(n).copied().unwrap_or("?"))
                };
                format!("L1:a value is unknown ({origin})")
            });
        }
    }
    // L2.
    if !v.full() {
        for (i, why) in m.family_reach.why.iter().enumerate() {
            if let Some(why) = why {
                v.mark(1 << i, || why.clone());
            }
        }
        observe_tokens(m, &mut v);
    }
    // L3.
    if !v.full() {
        text::scan(g, &mut |set, why| v.mark(set, why));
    }
    // L4: what a live family requires.
    loop {
        let before = v.live;
        for (i, f) in RUNTIME_FAMILIES.iter().enumerate() {
            if before & (1 << i) != 0 {
                for r in f.requires {
                    v.mark(families::family_bit(r), || {
                        format!("L4:required by {}", f.id)
                    });
                }
            }
        }
        if v.live == before {
            break;
        }
    }
    FamilyReach {
        families: RUNTIME_FAMILIES
            .iter()
            .zip(v.why)
            .map(|(f, why)| FamilyVerdict {
                family: f.id,
                live: why.is_some(),
                reason: why.unwrap_or_else(|| "none".into()),
            })
            .collect(),
    }
}
