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
    /// Diagnostics only (`PERRY_FACTS_EXPLAIN=1` or
    /// `PERRY_FACTS_FAMILY_REACH=closed`): what the fact would say were
    /// every value of the program known (the rules without L0 and L1). It
    /// names what a program in an unknown world observes anyway; it is no
    /// fact and nothing may act on it.
    pub closed: Vec<FamilyVerdict>,
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
            .chain(self.closed.iter().map(|f| {
                format!(
                    "# family_reach_closed {} {} reason={}",
                    f.family,
                    if f.live { "live" } else { "never" },
                    f.reason.replace(['\n', '\r'], " ")
                )
            }))
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

/// A prototype member by key: (the families of its holder, the families the
/// member adds, the entry).
pub(crate) type MemberKey = (FamilySet, FamilySet, &'static str);

/// What one realm entry observes.
#[derive(Clone, Copy, Default)]
struct Entry {
    /// Its own families.
    own: FamilySet,
    /// The families of what it declares born when it runs or is read as a
    /// member: its declared births that are not its own members. Empty for
    /// a holder of core (`String`, `process`): a core holder's declaration
    /// lists what any of its members may bear, and its members are observed
    /// one by one.
    births: FamilySet,
    /// The same without the exception for core holders: what the entry
    /// reaches when it is used as a value.
    reach: FamilySet,
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
    /// Entry -> its [[Prototype]] (`RealmEntry::proto`).
    protos: FxHashMap<&'static str, &'static str>,
    /// Root holder -> everything reachable below it: the families of its
    /// entries and of their declared births (the text part's closure).
    roots: FxHashMap<&'static str, FamilySet>,
    /// Key -> the prototype members under that key that add a family to
    /// their holder's (`normalize` on `String.prototype`, `toLocaleString` on
    /// `Date.prototype`): the holder's own families (empty for core: its
    /// values exist in every program), what the member adds, the entry.
    member_keys: FxHashMap<&'static str, Vec<MemberKey>>,
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
        use perry_hir::realm_manifest::RealmKind;
        let realm = crate::native_effects::realm();
        let mut ix = Index {
            entries: Default::default(),
            members: Default::default(),
            holders: Default::default(),
            protos: realm
                .iter()
                .filter_map(|e| Some((e.name, e.proto?)))
                .collect(),
            roots: Default::default(),
            member_keys: Default::default(),
            namespace_keys: Default::default(),
            computed: Vec::new(),
            string_proto: Vec::new(),
            number_proto: Vec::new(),
            bigint_proto: Vec::new(),
            array_proto: Vec::new(),
            function_proto: Vec::new(),
            object_proto: Vec::new(),
        };
        // A member of `parent` (structure, not something it bears).
        let child = |parent: &str, name: &str| {
            name.strip_prefix(parent)
                .is_some_and(|rest| rest.starts_with(['.', '[']))
        };
        for e in realm {
            let own = families_of(e.name);
            let reach = if is_global_holder(e.name) {
                0
            } else {
                e.births
                    .iter()
                    .filter(|b| !is_global_holder(b) && !child(e.name, b))
                    .fold(0, |set, b| set | families_of(b))
            };
            let core_holder = families_of(families::entry_root(e.name)) == 0
                && families::entry_module(e.name).is_none()
                && matches!(
                    e.kind,
                    RealmKind::Constructor
                        | RealmKind::Namespace
                        | RealmKind::Prototype
                        | RealmKind::Exotic
                );
            let births = if core_holder { 0 } else { reach };
            ix.entries.insert(e.name, Entry { own, births, reach });
        }
        // The global namespace objects (`Math`, `JSON`, `Intl`): lowering
        // can keep a read of a member of one as a read off the global scope.
        let namespaces: crate::pts::FxHashSet<&'static str> = realm
            .iter()
            .filter(|e| {
                matches!(e.kind, RealmKind::Namespace)
                    && families::entry_root(e.name) == e.name
                    && families::entry_module(e.name).is_none()
            })
            .map(|e| e.name)
            .collect();
        for e in realm {
            let Entry { own, reach, .. } = ix.entries[e.name];
            let root = families::entry_root(e.name);
            let root_own = families_of(root);
            let of_family = root_own != 0 || families::entry_module(e.name).is_some();
            *ix.roots.entry(root).or_default() |= own | reach;
            if of_family && own | reach != 0 {
                // A holder of a family: observing it observes the family
                // and what it declares born.
                ix.holders.insert(e.name, own | reach);
            }
            let Some((holder, key)) = split_member(e.name) else {
                continue;
            };
            if own | reach == 0 {
                continue;
            }
            ix.members
                .entry(holder)
                .or_default()
                .push((key, own | reach, e.name));
            // A prototype member that is of a family its holder is not, or
            // bears one. A prototype's values exist without the holder being
            // named, so the member's key is all the text has of it; a static
            // member (`process.on`, `Intl.Segmenter`) needs its holder named,
            // which the text rule's structural part observes.
            let beyond = (own | reach) & !root_own;
            let prototype = holder.contains(".prototype") || holder.starts_with('%');
            if beyond != 0 && prototype && families::entry_module(e.name).is_none() {
                ix.member_keys
                    .entry(key)
                    .or_default()
                    .push((root_own, beyond, e.name));
                // A core prototype's values are present in every program.
                if root_own == 0 {
                    ix.computed.push((beyond, e.name));
                }
            }
            if holder == root && namespaces.contains(root) && root_own != 0 {
                let slot = ix.namespace_keys.entry(key).or_insert((0, e.name));
                slot.0 |= own | reach;
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

    /// The families a computed key in a body that is not walked makes live.
    #[cfg(test)]
    pub(crate) fn computed_families(&self) -> FamilySet {
        self.computed.iter().fold(0, |set, (s, _)| set | s)
    }

    /// What running or reading entry `name` observes.
    pub(crate) fn entry(&self, name: &str) -> Option<FamilySet> {
        self.entries.get(name).map(|e| e.own | e.births)
    }

    /// What member `key` of entry `holder` observes, own or inherited
    /// through the realm's prototype chain (`performance.now` is
    /// `Performance.prototype.now`); `None`: no declared member.
    pub(crate) fn member(&self, holder: &str, key: &str) -> Option<FamilySet> {
        let mut holder = holder;
        for _ in 0..8 {
            if let Some(set) = self.entry(&format!("{holder}.{key}")) {
                return Some(set);
            }
            holder = self.protos.get(holder)?;
        }
        None
    }

    /// What naming entry `name` as a constructed class, a type or an
    /// operand observes: its families and what it declares born.
    pub(crate) fn named(&self, name: &str) -> Option<FamilySet> {
        self.entries.get(name).map(|e| e.own | e.reach)
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
    // An element or iteration key names no member.
    if members.is_empty() || m.is_elem_key(key) || m.is_iter_key(key) {
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
    m.family_reach.mark(e.births & !e.own, || {
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
    fn new() -> Self {
        Verdicts {
            live: 0,
            why: vec![None; RUNTIME_FAMILIES.len()],
        }
    }

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

    fn verdicts(self) -> Vec<FamilyVerdict> {
        RUNTIME_FAMILIES
            .iter()
            .zip(self.why)
            .map(|(f, why)| FamilyVerdict {
                family: f.id,
                live: why.is_some(),
                reason: why.unwrap_or_else(|| "none".into()),
            })
            .collect()
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
                    v.mark(e.births & !e.own, || {
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

/// L2, L3 and L4: what the program observes in a closed world.
fn closed_world(g: &crate::gen::Gen, dynamic_code: Option<&str>) -> Verdicts {
    let m = &g.m;
    let mut v = Verdicts::new();
    // `dyn_eval` is the driver's decision, an input of the solve.
    if let Some(linked) = dynamic_code {
        v.mark(families::family_bit("dyn_eval"), || {
            format!("input:{linked}")
        });
    }
    // L2.
    for (i, why) in m.family_reach.why.iter().enumerate() {
        if let Some(why) = why {
            v.mark(1 << i, || why.clone());
        }
    }
    observe_tokens(m, &mut v);
    // L3. A key of a prototype member of a family's holder counts once a
    // value of that holder can exist (the holder's family is live).
    let keys = if v.full() {
        Vec::new()
    } else {
        text::scan(g, &mut |set, why| v.mark(set, || why()))
    };
    // L4: what a live family requires.
    loop {
        let before = v.live;
        for &(holder, adds, entry) in &keys {
            if before & holder != 0 {
                v.mark(adds, || format!("L3:the key of {entry}"));
            }
        }
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
            return v;
        }
    }
}

/// The closed-world diagnostic is asked for.
fn closed_requested(m: &Model) -> bool {
    m.explain || std::env::var("PERRY_FACTS_FAMILY_REACH").as_deref() == Ok("closed")
}

/// The fact, after the fixpoint. `unit`: the modules are one unit of a
/// larger program (L0). `dynamic_code`: the driver linked the interpreter
/// (an input of the solve, reported as such).
pub(crate) fn derive(g: &crate::gen::Gen, unit: bool, dynamic_code: Option<&str>) -> FamilyReach {
    let m = &g.m;
    let mut v = Verdicts::new();
    // L0.
    if unit {
        v.all(|| "L0:per-unit solve".into());
    } else if m.aborted {
        v.all(|| "L0:solve aborted".into());
    } else if m.widened {
        v.all(|| "L0:solve widened".into());
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
        } else if let Some(why) = unknown_value(m) {
            v.all(|| why);
        }
    }
    // L2, L3, L4.
    let diagnostic = closed_requested(m) && !unit && !m.aborted;
    let closed = (!v.full() || diagnostic).then(|| closed_world(g, dynamic_code));
    if let Some(closed) = &closed {
        for (i, why) in closed.why.iter().enumerate() {
            if let Some(why) = why {
                v.mark(1 << i, || why.clone());
            }
        }
    }
    FamilyReach {
        families: v.verdicts(),
        closed: closed
            .filter(|_| diagnostic)
            .map(Verdicts::verdicts)
            .unwrap_or_default(),
    }
}

/// L1's "some node holds ⊤": the reason, naming where. Every unknown value
/// enters the solve as ⊤ (`Model::add_top`, or an edge from the shared ⊤
/// source `Model::top_tok_node`, which holds ⊤ in every program and is no
/// value of this one).
fn unknown_value(m: &Model) -> Option<String> {
    let mut labels = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    for (i, n) in m.nodes.iter().enumerate() {
        if i as u32 != m.top_tok_node && n.pts.contains_tok(TOP) {
            let label = m.labels.get(i).copied().unwrap_or("?");
            labels.entry(label).or_insert((i, 0)).1 += 1;
        }
    }
    let (&label, &(first, _)) = labels.iter().next()?;
    let nodes: usize = labels.values().map(|v| v.1).sum();
    let origin = if m.explain {
        format!(": {}", m.top_origin(first as u32))
    } else {
        String::new()
    };
    let kinds: Vec<String> = labels
        .iter()
        .take(6)
        .map(|(l, (_, n))| format!("{l} x{n}"))
        .collect();
    Some(format!(
        "L1:a value is unknown ({nodes} nodes hold it, first at {label}{origin}; {})",
        kinds.join(", ")
    ))
}
