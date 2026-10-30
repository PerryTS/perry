//! Object.create(P) families (OBJECT_MODEL_2026-10-13.md §3.2, §3.6).
//!
//! [`collect`] derives prototype facts from module HIR; [`CreateState`] follows
//! bindings and subsequent stores, while [`site`] reads Object.assign literals.
//! Fixed layouts name ordered keys and P's identity, shared by matching births
//! and emitted by [`emission`]. Imported keys come from [`program_proto_exports`].
//!
//! ProtoCensus::create_add_free proves keys cannot be intercepted; otherwise
//! [`emit_chain_proof`] checks carried prototype layouts from [`image`] before
//! stores. Mutated chains, unpredictable initializers and changed spread sources
//! decline to ordinary [[Set]]. perry_bind_family_prototype binds and validates
//! P's image and serial; perry_create_family checks that serial, falling back to
//! ordinary Object.create and [[Set]] when it differs. Facts never outlive module
//! lowering and no lookup depends on an expression's address.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, RwLock};

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, Module, Stmt};

use crate::expr::FnCtx;
use crate::rooting::{self, Repr};
use crate::types::{DOUBLE, I32, I64, I8, PTR};

/// One family layout.
#[derive(Clone, Debug)]
pub(crate) struct Site {
    /// The family's carried descriptor.
    pub desc_global: String,
    /// Its keys, in order (slot = position).
    pub keys: Vec<String>,
    /// Its slot count (at least one, at least the key count).
    pub slots: u32,
    /// The `PerryProtoFamily` of its prototype binding.
    pub proto_global: String,
}

/// A `const a = Object.create(P)` creation point's family: born with one
/// slot for each key the statements after it store on `a`, in source order,
/// and one state per prefix of them (§3.2, as `layout_desc::FamilyAdd` for
/// literals).
#[derive(Clone, Debug)]
pub(crate) struct Create {
    /// `states[i]` holds `keys[..i]`; every state has `keys.len()` slots,
    /// and `states[0]` is the birth.
    pub states: Vec<String>,
    pub keys: Vec<String>,
    /// `P`'s binding.
    pub proto: u32,
    pub proto_global: String,
    /// The checked proof (empty when the census proves every add free): each
    /// prototype binding of `P`'s chain with its carried layout's
    /// descriptor, `P` first.
    pub chain: Vec<(u32, String)>,
    /// The chain ends at `%Object.prototype%` (else at null).
    pub object_root: bool,
}

/// One add of a [`Create`] family: its key `keys[index]`.
#[derive(Clone, Debug)]
pub(crate) struct Add {
    pub family: Create,
    pub index: usize,
}

/// The create families of the body being lowered (a field of its `FnCtx`):
/// the family of the `let` statement being lowered, until its
/// `Object.create` takes it, and the families of the bindings born so far.
#[derive(Clone, Debug, Default)]
pub(crate) struct CreateState {
    pending: Option<(u32, Create)>,
    born: HashMap<u32, Create>,
    /// The method call being lowered reads its method through
    /// [`holder_read`]: set around its dispatch, taken by the method site.
    pub(crate) method_holder: Option<HolderRead>,
}

/// `a.k` on a binding born of a family, read through the carried layouts
/// of its prototype chain (§3.6): `a` in one of the family's states (none
/// holds `k`) with no meta record, so it has no own `k` and its
/// [[Prototype]] is `P`; each prototype from `P` up to the first whose
/// carried layout holds `k` as data (the holder) still on that layout, so
/// none before it has `k` and the holder's `k` is its slot. One compare of a
/// layout word per object, no chain walked at run time.
#[derive(Clone, Debug)]
pub(crate) struct HolderRead {
    states: Vec<String>,
    /// Each prototype binding from `P` to the holder, with its carried
    /// layout's descriptor.
    chain: Vec<(u32, String)>,
    /// The holder's slot of `k`.
    slot: u32,
}

/// A key of a carried prototype layout, as the runtime names it: a string,
/// a well-known symbol (its number in `perry_abi::WELL_KNOWN_SYMBOLS`), a
/// registered symbol (`Symbol.for` of the string) or the symbol a top-level
/// `const k = Symbol(…)` of this module binds (`Local(k)`: its value is
/// handed to the prototype's bind, [`lower_binding`]; never an export fact).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImageKey {
    Str(String),
    WellKnown(u32),
    Registered(String),
    Local(u32),
}

impl ImageKey {
    fn is_symbol(&self) -> bool {
        !matches!(self, ImageKey::Str(_))
    }
}

/// Is a key fact meaningful in another module (not a binding of this one)?
fn exportable(key: &ImageKey) -> bool {
    !matches!(key, ImageKey::Local(_))
}

/// One key of a carried prototype layout: the key and its attribute entry
/// (`perry_abi::LAYOUT_KEY_*`, the runtime's `key_attrs` entry bits).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageEntry {
    pub key: ImageKey,
    pub attrs: u8,
}

/// Attribute entry bits (`perry_abi`, pinned to the runtime's `key_attrs`).
const NON_WRITABLE: u8 = crate::runtime_abi::LAYOUT_KEY_NON_WRITABLE;
const NON_ENUMERABLE: u8 = crate::runtime_abi::LAYOUT_KEY_NON_ENUMERABLE;
const NON_CONFIGURABLE: u8 = crate::runtime_abi::LAYOUT_KEY_NON_CONFIGURABLE;
const ACCESSOR: u8 = crate::runtime_abi::LAYOUT_KEY_ACCESSOR;
const HAS_GET: u8 = crate::runtime_abi::LAYOUT_KEY_HAS_GET;
const HAS_SET: u8 = crate::runtime_abi::LAYOUT_KEY_HAS_SET;

/// Is the entry a writable data property (an add on a child shadows it)?
fn plain_data(attrs: u8) -> bool {
    attrs & (ACCESSOR | NON_WRITABLE) == 0
}

/// What another module's export is at compile time: a property key (a
/// string constant or `Symbol.for` of one), or a literal's own keys (its
/// [`image`]). Keyed by (defining module path, export name).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExportFact {
    Key(ImageKey),
    Image(Vec<ImageEntry>),
}

/// The program's export facts, by (module path, export name).
pub type ProtoExports = BTreeMap<(String, String), ExportFact>;

mod proto_exports;
use proto_exports::program_exports;
pub use proto_exports::{program_proto_exports, program_proto_exports_key, set_program_proto_exports};

/// What a prototype binding's initializer makes (§3.6): which keys of the
/// object are, or may be, accessors or non-writable, and its [[Prototype]].
/// Every other key it holds, named or not (a spread, a computed key), is a
/// plain writable data property, which an add shadows.
#[derive(Clone, Debug, Default)]
struct Born {
    blocks: BTreeSet<String>,
    proto: Link,
    /// An `Object.assign` source key the compiler cannot name may have
    /// relinked it (an own `__proto__` data key), which the census does not
    /// see: no family stands on its chain.
    dynamic: bool,
    /// Its carried layout: every own key in order with its attributes, when
    /// the compiler can name them all (else `None`).
    image: Option<Vec<ImageEntry>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Link {
    /// `%Object.prototype%`.
    #[default]
    Object,
    Null,
    /// Another prototype binding of the module.
    Binding(u32),
}

/// What a computed literal key is known to be.
#[derive(Clone, Debug)]
enum KeyConst {
    Str(String),
    /// A symbol: which one, when the compiler can name it.
    Symbol(Option<ImageKey>),
}

/// `Symbol.for(desc)` as a layout key: a well-known symbol (the HIR spells
/// `Symbol.iterator` as `Symbol.for("@@__perry_wk_iterator")`) or a
/// registered one.
fn symbol_key(desc: &str) -> Option<ImageKey> {
    match desc.strip_prefix("@@__perry_wk_") {
        Some(name) => crate::runtime_abi::WELL_KNOWN_SYMBOLS
            .iter()
            .position(|n| *n == name)
            .map(|i| ImageKey::WellKnown(i as u32)),
        None => Some(ImageKey::Registered(desc.to_string())),
    }
}

/// `%Object.prototype%`'s own keys: data methods an add of the name would
/// shadow, and `__proto__`. A family key is none of them (an intrinsic's
/// attributes are not the compiler's to follow).
const OBJECT_PROTOTYPE_KEYS: &[&str] = &[
    "__proto__",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// The module's prototype facts, derived once from its HIR, and the
/// layouts its lowered code used (the emission's input).
#[derive(Default)]
struct ModuleCreateFamilies {
    prefix: String,
    /// Prototype bindings the compiler follows.
    borns: HashMap<u32, Born>,
    /// Those bound to a closed literal (an `Object.assign` site's `P`).
    literals: BTreeSet<u32>,
    /// Followed prototypes some `Object.create` names (either creation
    /// point): their `let` binds the family identity.
    bound: BTreeSet<u32>,
    /// The layouts lowering used, by descriptor name.
    used: BTreeMap<String, Site>,
    /// Every layout lowering could use (a superset of `used`): the births
    /// pre-pass reserves their ordinals because it does not lower bodies.
    candidates: BTreeMap<String, Site>,
    /// Checked proofs are emitted (not on 32-bit targets).
    checked: bool,
}

thread_local! {
    static MODULE_CREATE_FAMILIES: RefCell<ModuleCreateFamilies> =
        RefCell::new(ModuleCreateFamilies::default());
}

/// Forget the previous module's families.
pub(crate) fn clear() {
    MODULE_CREATE_FAMILIES.with(|m| *m.borrow_mut() = ModuleCreateFamilies::default());
}

/// A key a layout slot may carry and `Object.assign` adds in source order:
/// an atom, not an array index (enumerated first), not `__proto__`.
fn layout_key(key: &str) -> bool {
    super::key_is_atom(key)
        && key != "__proto__"
        && !(key.bytes().all(|b| b.is_ascii_digit()) && (key == "0" || !key.starts_with('0')))
}

/// May a family birth hold `key` without the ordinary `[[Set]]`: no
/// prototype of the family can intercept its add (the census), and adding it
/// would not set the shadow bit (it is no prototype name).
fn add_free(key: &str) -> bool {
    layout_key(key)
        && crate::proto_constants::create_add_free(key)
        && !crate::proto_constants::is_prototype_name(key)
}

/// The closed-literal class `e` allocates, if any.
fn literal_class<'a>(classes: &[&'a perry_hir::Class], e: &Expr) -> Option<&'a perry_hir::Class> {
    let Expr::New { class_name, .. } = e else {
        return None;
    };
    classes
        .iter()
        .find(|c| &c.name == class_name)
        .copied()
        .filter(|c| c.is_literal_shape())
}

/// The prototype binding `Object.create`'s argument reads: `P`, or `P`
/// behind its TDZ check (a hoisted function's read).
fn proto_binding(proto: &Expr) -> Option<u32> {
    match proto {
        Expr::LocalGet(p) => Some(*p),
        Expr::Sequence(parts) => match parts.as_slice() {
            [Expr::Call { callee, args, .. }, Expr::LocalGet(p)]
                if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. }
                    if name == "js_tdz_check_binding")
                    && matches!(args.first(), Some(Expr::LocalGet(x)) if x == p) =>
            {
                Some(*p)
            }
            _ => None,
        },
        _ => None,
    }
}

/// One module's prototype facts (its own, and its imports' through the
/// program's export facts).
struct Analysis<'a> {
    classes: Vec<&'a perry_hir::Class>,
    consts: HashMap<u32, KeyConst>,
    borns: HashMap<u32, Born>,
    literals: BTreeSet<u32>,
    /// Top-level `const` bindings bound once, by name.
    top_level: HashMap<&'a str, u32>,
    /// Import local name → (defining module path, export name), `None` for
    /// a namespace import.
    imports: HashMap<&'a str, (&'a str, Option<&'a str>)>,
    exports: &'a ProtoExports,
}

impl Analysis<'_> {
    /// The export fact an imported binding (`ExternFuncRef`) or a
    /// namespace member (`ns.name`) reads.
    fn imported(&self, e: &Expr) -> Option<&ExportFact> {
        let (local, member) = match e {
            Expr::ExternFuncRef { name, .. } => (name.as_str(), None),
            Expr::PropertyGet {
                object, property, ..
            } => match object.as_ref() {
                Expr::ExternFuncRef { name, .. } => (name.as_str(), Some(property.as_str())),
                _ => return None,
            },
            _ => return None,
        };
        let (path, name) = match (self.imports.get(local)?, member) {
            ((path, Some(name)), None) => (*path, *name),
            ((path, None), Some(member)) => (*path, member),
            _ => return None,
        };
        self.exports.get(&(path.to_string(), name.to_string()))
    }

    /// A computed key `e` names.
    fn key_const(&self, e: &Expr) -> Option<KeyConst> {
        match e {
            Expr::String(s) => Some(KeyConst::Str(s.clone())),
            Expr::SymbolFor(desc) => Some(KeyConst::Symbol(match desc.as_ref() {
                Expr::String(d) => symbol_key(d),
                _ => None,
            })),
            Expr::LocalGet(x) => self.consts.get(x).cloned(),
            _ => match self.imported(e)? {
                ExportFact::Key(ImageKey::Str(s)) => Some(KeyConst::Str(s.clone())),
                ExportFact::Key(k) => Some(KeyConst::Symbol(Some(k.clone()))),
                ExportFact::Image(_) => None,
            },
        }
    }

    /// A string constant expression: a key constant that is a string, or a
    /// concatenation of them (`a + b`, a template literal's parts, whose
    /// substitutions are `TemplateStringCoerce`). Every operand is a string,
    /// so `+` concatenates and ToString is the identity.
    fn str_const(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Binary {
                op: BinaryOp::Add,
                left,
                right,
            } => {
                let mut s = self.str_const(left)?;
                s.push_str(&self.str_const(right)?);
                Some(s)
            }
            Expr::TemplateStringCoerce(x) => self.str_const(x),
            _ => match self.key_const(e)? {
                KeyConst::Str(s) => Some(s),
                KeyConst::Symbol(_) => None,
            },
        }
    }

    /// The own keys of a spread or assign source `e`, when known.
    fn source_image(&self, e: &Expr) -> Option<Vec<ImageEntry>> {
        match e {
            Expr::LocalGet(x) => self.borns.get(x)?.image.clone(),
            _ => match self.imported(e) {
                Some(ExportFact::Image(image)) => Some(image.clone()),
                Some(ExportFact::Key(_)) => None,
                None => literal(self, e).and_then(|l| l.image),
            },
        }
    }
}

/// Derive one module's prototype facts: one pass over its top-level
/// bindings, in initialization order (a chain names only earlier ones).
fn analyze<'a>(hir: &'a Module, exports: &'a ProtoExports) -> Analysis<'a> {
    let mut bound_once: HashMap<u32, usize> = HashMap::new();
    for stmt in &hir.init {
        if let Stmt::Let {
            id,
            mutable: false,
            init: Some(_),
            ..
        } = stmt
        {
            *bound_once.entry(*id).or_default() += 1;
        }
    }
    let mut imports = HashMap::new();
    for import in &hir.imports {
        let Some(path) = import.resolved_path.as_deref() else {
            continue;
        };
        for spec in &import.specifiers {
            match spec {
                perry_hir::ImportSpecifier::Named { imported, local } => {
                    imports.insert(local.as_str(), (path, Some(imported.as_str())));
                }
                perry_hir::ImportSpecifier::Namespace { local } => {
                    imports.insert(local.as_str(), (path, None));
                }
                perry_hir::ImportSpecifier::Default { .. } => {}
            }
        }
    }
    let mut a = Analysis {
        classes: hir.classes.iter().collect(),
        consts: HashMap::new(),
        borns: HashMap::new(),
        literals: BTreeSet::new(),
        top_level: HashMap::new(),
        imports,
        exports,
    };
    for stmt in &hir.init {
        let Stmt::Let {
            id,
            name,
            mutable: false,
            init: Some(init),
            ..
        } = stmt
        else {
            continue;
        };
        if bound_once.get(id) != Some(&1) {
            continue;
        }
        a.top_level.insert(name.as_str(), *id);
        // `const k = Symbol(…)`: one symbol, the binding's for its lifetime.
        if matches!(init, Expr::SymbolNew(_)) {
            a.consts
                .insert(*id, KeyConst::Symbol(Some(ImageKey::Local(*id))));
            continue;
        }
        if matches!(init, Expr::String(_) | Expr::SymbolFor(_)) {
            if let Some(k) = a.key_const(init) {
                a.consts.insert(*id, k);
            }
            continue;
        }
        if matches!(
            init,
            Expr::Binary {
                op: BinaryOp::Add,
                ..
            }
        ) {
            if let Some(s) = a.str_const(init) {
                a.consts.insert(*id, KeyConst::Str(s));
            }
            continue;
        }
        if let Some(k) = a.imported(init).and_then(|f| match f {
            ExportFact::Key(ImageKey::Str(s)) => Some(KeyConst::Str(s.clone())),
            ExportFact::Key(k) => Some(KeyConst::Symbol(Some(k.clone()))),
            ExportFact::Image(_) => None,
        }) {
            a.consts.insert(*id, k);
            continue;
        }
        if literal_class(&a.classes, init).is_some() {
            a.literals.insert(*id);
        }
        if let Some(mut b) = born(&a, init) {
            // A carried layout's string keys are atoms its slots carry.
            if b.image.as_ref().is_some_and(|img| {
                img.iter()
                    .any(|e| matches!(&e.key, ImageKey::Str(k) if !layout_key(k)))
            }) {
                b.image = None;
            }
            a.borns.insert(*id, b);
        }
    }
    a
}

/// A class field whose computed key names a string constant (`[K] = …`,
/// `K` a string constant of the module or one it imports, read with no TDZ
/// check) is the field named by that string: its key, evaluated once at the
/// class definition, is that string. Rename each such field of `hir`, so
/// every consumer of class keys (field definition order, class layouts,
/// families, slot indexes) sees one named field at its declared position.
/// Driver, after [`set_program_proto_exports`] and before stubs and codegen.
pub fn name_computed_class_fields(hir: &mut Module) {
    let exports = program_exports();
    let mut named: Vec<(u32, usize, String)> = Vec::new();
    {
        let a = analyze(hir, &exports);
        for c in &hir.classes {
            for (i, f) in c.fields.iter().enumerate() {
                if !f.decorators.is_empty() || f.is_private {
                    continue;
                }
                if let Some(KeyConst::Str(k)) = f.key_expr.as_ref().and_then(|e| a.key_const(e)) {
                    // An index key is an element; `constructor` and
                    // `__proto__` are no plain field names.
                    if !k.is_empty()
                        && !k.bytes().all(|b| b.is_ascii_digit())
                        && k != "constructor"
                        && k != "__proto__"
                    {
                        named.push((c.id, i, k));
                    }
                }
            }
        }
    }
    for (class_id, index, key) in named {
        if let Some(f) = hir
            .classes
            .iter_mut()
            .find(|c| c.id == class_id)
            .and_then(|c| c.fields.get_mut(index))
        {
            f.name = key;
            f.key_expr = None;
        }
    }
}

/// Derive the module's prototype facts (`codegen/mod.rs`, executables with
/// layouts only), and which of them some `Object.create` names.
pub(crate) fn collect(hir: &Module, module_prefix: &str, target_triple: &str) {
    clear();
    let exports = program_exports();
    let a = analyze(hir, &exports);
    let mut state = ModuleCreateFamilies {
        prefix: module_prefix.to_string(),
        borns: a.borns,
        literals: a.literals,
        checked: !crate::target_layout::target_is_ilp32(target_triple),
        ..ModuleCreateFamilies::default()
    };
    if !state.borns.is_empty() {
        perry_hir::for_each_module_expr(hir, &mut |e| {
            if let Expr::ObjectCreate(proto, None) = e {
                if let Some(p) = proto_binding(proto) {
                    if state.borns.contains_key(&p) {
                        state.bound.insert(p);
                    }
                }
            }
        });
    }
    if !state.borns.is_empty() {
        state.candidates = candidate_sites(hir, &state);
    }
    MODULE_CREATE_FAMILIES.with(|m| *m.borrow_mut() = state);
}

/// Every layout the module's creation points and literal adds could use:
/// the states of each `const a = Object.create(P)` family
/// ([`before_stmt`]) and each literal add's layout ([`site`]).
fn candidate_sites(hir: &Module, m: &ModuleCreateFamilies) -> BTreeMap<String, Site> {
    let mut out = BTreeMap::new();
    perry_hir::tdz_check::for_each_module_stmt_list(hir, &mut |stmts| {
        for (i, stmt) in stmts.iter().enumerate() {
            let Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::ObjectCreate(proto, None)),
                ..
            } = stmt
            else {
                continue;
            };
            let Some(p) = proto_binding(proto) else {
                continue;
            };
            let Some(family) = binding_family(m, *id, p, &stmts[i + 1..]) else {
                continue;
            };
            for (n, desc) in family.states.iter().enumerate() {
                out.entry(desc.clone()).or_insert_with(|| Site {
                    desc_global: desc.clone(),
                    keys: family.keys[..n].to_vec(),
                    slots: family.keys.len() as u32,
                    proto_global: family.proto_global.clone(),
                });
            }
        }
    });
    perry_hir::for_each_module_expr(hir, &mut |e| {
        let Expr::ObjectAssign { target, sources } = e else {
            return;
        };
        let Expr::ObjectCreate(proto, None) = target.as_ref() else {
            return;
        };
        let (
            Some(p),
            [Expr::New {
                class_name, args, ..
            }],
        ) = (proto_binding(proto), sources.as_slice())
        else {
            return;
        };
        let Some(class) = hir
            .classes
            .iter()
            .find(|c| &c.name == class_name)
            .filter(|c| c.is_literal_shape())
        else {
            return;
        };
        if let Some(site) = literal_add_site(m, p, class, args.len()) {
            out.entry(site.desc_global.clone()).or_insert(site);
        }
    });
    out
}

/// Must the top-level binding `id` be a module global (a prototype born on
/// its carried layout, whose word a family add compares)?
pub(crate) fn binding_needs_global(id: u32) -> bool {
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        m.checked && m.bound.contains(&id) && m.borns.get(&id).is_some_and(|b| b.image.is_some())
    })
}

/// The descriptor of prototype binding `p`'s carried layout.
fn image_global(prefix: &str, p: u32) -> String {
    format!("perry_layout_pimg_{prefix}_{p}")
}

/// May an add of `key` shadow every image of `chain` (absent, or a writable
/// data property)?
fn images_admit(m: &ModuleCreateFamilies, chain: &[u32], key: &str) -> bool {
    chain.iter().all(|h| {
        m.borns[h].image.as_ref().is_some_and(|img| {
            img.iter()
                .all(|e| e.key != ImageKey::Str(key.to_string()) || plain_data(e.attrs))
        })
    })
}

/// The family of `const a = Object.create(P)`, from the statements `rest`
/// after it: the longest prefix of the keys they store on `a` whose adds
/// the census proves free on `P`'s chain, or, when every prototype of the
/// chain is born on its carried layout, whose adds that layout admits (the
/// checked proof).
fn binding_family(m: &ModuleCreateFamilies, a: u32, p: u32, rest: &[Stmt]) -> Option<Create> {
    let chain = chain_of(p, &m.borns)?;
    let carried = m.checked
        && chain
            .iter()
            .all(|h| m.bound.contains(h) && m.borns[h].image.is_some());
    if !carried && chain.iter().any(|h| m.borns[h].dynamic) {
        return None;
    }
    let mut keys: Vec<String> = Vec::new();
    let mut checked = false;
    for stmt in rest {
        let Stmt::Expr(e) = stmt else { continue };
        let Some((target, key)) = static_store(e) else {
            continue;
        };
        if target != a || keys.iter().any(|k| k == key) {
            continue;
        }
        if keys.len() == MAX_CREATE_KEYS
            || !layout_key(key)
            || crate::proto_constants::is_prototype_name(key)
            || OBJECT_PROTOTYPE_KEYS.contains(&key)
        {
            break;
        }
        let free = !chain.iter().any(|h| m.borns[h].dynamic)
            && crate::proto_constants::create_add_free(key)
            && !chain.iter().any(|h| m.borns[h].blocks.contains(key));
        if !free {
            if !carried || !images_admit(m, &chain, key) {
                break;
            }
            checked = true;
        }
        keys.push(key.to_string());
    }
    if keys.is_empty() {
        return None;
    }
    let family = format!(
        "{}_{p}_{:08x}",
        m.prefix,
        super::fnv1a(keys.join("\u{1}").as_bytes()) as u32
    );
    let root = m.borns[chain.last()?].proto;
    Some(Create {
        states: (0..=keys.len())
            .map(|i| format!("perry_layout_cf_{family}_{i}"))
            .collect(),
        keys,
        proto: p,
        proto_global: format!("perry_protofam_{}_{p}", m.prefix),
        chain: if checked {
            chain
                .iter()
                .map(|h| (*h, image_global(&m.prefix, *h)))
                .collect()
        } else {
            Vec::new()
        },
        object_root: root == Link::Object,
    })
}

/// Statement lowering, before `stmts[i]`: a `const a = Object.create(P)`
/// whose family the statements after it make is the next creation point.
pub(crate) fn before_stmt(ctx: &mut FnCtx<'_>, stmts: &[Stmt], i: usize) {
    ctx.create_families.pending = None;
    let Stmt::Let {
        id,
        mutable: false,
        init: Some(Expr::ObjectCreate(proto, None)),
        ..
    } = &stmts[i]
    else {
        return;
    };
    let Some(p) = proto_binding(proto) else {
        return;
    };
    let family = MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        binding_family(&m, *id, p, &stmts[i + 1..])
    });
    ctx.create_families.pending = family.map(|f| (*id, f));
}

/// Statement lowering, after a statement: a creation point not lowered by
/// [`lower_create`] makes no family.
pub(crate) fn after_stmt(ctx: &mut FnCtx<'_>) {
    ctx.create_families.pending = None;
}

/// Record that lowered code uses `site`'s layout.
fn use_layout(site: Site) {
    MODULE_CREATE_FAMILIES.with(|m| {
        m.borrow_mut()
            .used
            .entry(site.desc_global.clone())
            .or_insert(site);
    });
}

/// The family whose birth the `Object.create` expression `e` is: the
/// pending creation point's, when `e` is its initializer. Its layouts are
/// used from here on, and the binding's stores find it.
pub(crate) fn create_site(ctx: &mut FnCtx<'_>, e: &Expr) -> Option<Create> {
    let Expr::ObjectCreate(proto, None) = e else {
        return None;
    };
    let p = proto_binding(proto)?;
    if ctx
        .create_families
        .pending
        .as_ref()
        .is_none_or(|(_, f)| f.proto != p)
    {
        return None;
    }
    let (a, family) = ctx.create_families.pending.take()?;
    for (i, desc) in family.states.iter().enumerate() {
        use_layout(Site {
            desc_global: desc.clone(),
            keys: family.keys[..i].to_vec(),
            slots: family.keys.len() as u32,
            proto_global: family.proto_global.clone(),
        });
    }
    ctx.create_families.born.insert(a, family.clone());
    Some(family)
}

/// The family add `object.property = …` names, when `object` is a binding
/// born of a family that holds `property`.
pub(crate) fn add_site(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> Option<Add> {
    let Expr::LocalGet(a) = object else {
        return None;
    };
    let family = ctx.create_families.born.get(a)?;
    let index = family.keys.iter().position(|k| k == property)?;
    Some(Add {
        family: family.clone(),
        index,
    })
}

/// The [`HolderRead`] of `object.property`, when `object` is a binding
/// born of a family whose chain's carried layouts hold `property` as data.
pub(crate) fn holder_read(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> Option<HolderRead> {
    let Expr::LocalGet(a) = object else {
        return None;
    };
    let family = ctx.create_families.born.get(a)?;
    if property == "__proto__" || family.keys.iter().any(|k| k == property) {
        return None;
    }
    let key = ImageKey::Str(property.to_string());
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        if !m.checked {
            return None;
        }
        let mut chain = Vec::new();
        for h in chain_of(family.proto, &m.borns)? {
            let born = &m.borns[&h];
            // Emitted with its descriptor (`emission`), and born there.
            let linked = match born.proto {
                Link::Binding(q) => m.bound.contains(&q),
                Link::Object | Link::Null => true,
            };
            if !linked || !m.bound.contains(&h) || !ctx.module_globals.contains_key(&h) {
                return None;
            }
            let image = born.image.as_ref()?;
            chain.push((h, image_global(&m.prefix, h)));
            if let Some(pos) = image.iter().position(|e| e.key == key) {
                if image[pos].attrs & ACCESSOR != 0 {
                    return None;
                }
                return Some(HolderRead {
                    states: family.states.clone(),
                    chain,
                    slot: pos as u32,
                });
            }
        }
        None
    })
}

/// The proof of a [`HolderRead`] for the object pointer `biased` (biased
/// as `receiver_range` makes it): branches to `miss_l` unless it holds, and
/// returns the holder slot's value bits (`i64`, never the hole).
pub(crate) fn emit_holder_load(
    ctx: &mut FnCtx<'_>,
    read: &HolderRead,
    biased: &str,
    miss_l: &str,
) -> String {
    use crate::expr::receiver_range::emit_field_ptr;
    let header =
        crate::target_layout::object_slot_base_bytes(ctx.target_triple, read.form()) as i64;
    let ids: Vec<String> = read.states.iter().map(|s| layout_id(ctx, s)).collect();
    let chain_ids: Vec<String> = read.chain.iter().map(|(_, d)| layout_id(ctx, d)).collect();
    let mut ok = "false".to_string();
    {
        let blk = ctx.block();
        let wp = emit_field_ptr(blk, biased, 4);
        let word = blk.load(I32, &wp);
        for id in &ids {
            let registered = blk.icmp_ne(I32, id, "0");
            let same = blk.icmp_eq(I32, &word, id);
            let held = blk.and(crate::types::I1, &registered, &same);
            ok = blk.or(crate::types::I1, &ok, &held);
        }
        let mp = emit_field_ptr(blk, biased, header - 8);
        let meta = blk.load(PTR, &mp);
        let no_meta = blk.icmp_eq(PTR, &meta, "null");
        ok = blk.and(crate::types::I1, &ok, &no_meta);
    }
    let mut holder = String::new();
    for ((h, _), id) in read.chain.iter().zip(&chain_ids) {
        let global = ctx.module_globals[h].clone();
        let next = ctx.new_block("cholder.image");
        let next_l = ctx.block_label(next);
        ctx.block().cond_br(&ok, &next_l, miss_l);
        ctx.current_block = next;
        let blk = ctx.block();
        let value = blk.load(DOUBLE, &format!("@{global}"));
        let bits = blk.bitcast_double_to_i64(&value);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let ptr = blk.inttoptr(I64, &handle);
        let word_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
        let word = blk.load(I32, &word_ptr);
        let registered = blk.icmp_ne(I32, id, "0");
        let same = blk.icmp_eq(I32, &word, id);
        ok = blk.and(crate::types::I1, &registered, &same);
        holder = ptr;
    }
    let hit = ctx.new_block("cholder.image_hit");
    let hit_l = ctx.block_label(hit);
    ctx.block().cond_br(&ok, &hit_l, miss_l);
    ctx.current_block = hit;
    let blk = ctx.block();
    let offset = (header + 8 * i64::from(read.slot)).to_string();
    let sp = blk.gep(I8, &holder, &[(I64, &offset)]);
    let v = blk.load(I64, &sp);
    let hole = blk.icmp_eq(I64, &v, crate::nanbox::TAG_HOLE_I64);
    let value_idx = ctx.new_block("cholder.image_value");
    let value_l = ctx.block_label(value_idx);
    ctx.block().cond_br(&hole, miss_l, &value_l);
    ctx.current_block = value_idx;
    v
}

/// `object.property` through [`holder_read`] (`expr/dispatch.rs`), else
/// `None`. Any receiver the proof refuses takes the ordinary read.
pub(crate) fn lower_holder_read(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    read: &Expr,
) -> Option<Result<String>> {
    let site = holder_read(ctx, object, property)?;
    Some((|| {
        let recv = crate::expr::lower_expr(ctx, object)?;
        let check = ctx.new_block("cholder.recv");
        let slow = ctx.new_block("cholder.generic");
        let merge = ctx.new_block("cholder.merge");
        let check_l = ctx.block_label(check);
        let slow_l = ctx.block_label(slow);
        let merge_l = ctx.block_label(merge);
        let biased = {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&recv);
            let fused = crate::expr::receiver_range::emit_fused_receiver_test(blk, &bits);
            blk.cond_br(&fused.is_object_pointer, &check_l, &slow_l);
            fused.biased
        };
        ctx.current_block = check;
        let v = emit_holder_load(ctx, &site, &biased, &slow_l);
        let hit = ctx.block().bitcast_i64_to_double(&v);
        let hit_end = ctx.block_label(ctx.current_block);
        ctx.block().br(&merge_l);
        ctx.current_block = slow;
        let generic = crate::expr::property_get::lower(ctx, read)?;
        let slow_end = ctx.block_label(ctx.current_block);
        ctx.block().br(&merge_l);
        ctx.current_block = merge;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&hit, &hit_end), (&generic, &slow_end)]))
    })())
}

/// The family of `Object.assign(Object.create(P), { k1: e1, … })`, when `P`
/// is bound to a closed literal and every key's add is free.
/// The layout of `Object.assign(Object.create(P), new Literal(..))` when `P`
/// is a closed literal and the literal's keys are free adds.
fn literal_add_site(
    m: &ModuleCreateFamilies,
    p: u32,
    class: &perry_hir::Class,
    arg_count: usize,
) -> Option<Site> {
    let keys: Vec<String> = class.fields.iter().map(|f| f.name.clone()).collect();
    if keys.is_empty()
        || keys.len() != arg_count
        || class.fields.iter().any(|f| f.key_expr.is_some())
        || !keys.iter().all(|k| layout_key(k))
        || (1..keys.len()).any(|i| keys[..i].contains(&keys[i]))
        || !keys.iter().all(|k| add_free(k))
    {
        return None;
    }
    m.literals.contains(&p).then(|| Site {
        desc_global: format!(
            "perry_layout_create_{}_{p}_{:08x}",
            m.prefix,
            super::fnv1a(keys.join("\u{1}").as_bytes()) as u32
        ),
        slots: keys.len() as u32,
        keys,
        proto_global: format!("perry_protofam_{}_{p}", m.prefix),
    })
}

pub(crate) fn site(ctx: &FnCtx<'_>, e: &Expr) -> Option<Site> {
    let Expr::ObjectAssign { target, sources } = e else {
        return None;
    };
    let Expr::ObjectCreate(proto, None) = target.as_ref() else {
        return None;
    };
    let p = proto_binding(proto)?;
    let [source] = sources.as_slice() else {
        return None;
    };
    let Expr::New {
        class_name, args, ..
    } = source
    else {
        return None;
    };
    let class = ctx
        .classes
        .get(class_name)
        .filter(|c| c.is_literal_shape())?;
    let site =
        MODULE_CREATE_FAMILIES.with(|m| literal_add_site(&m.borrow(), p, class, args.len()))?;
    use_layout(site.clone());
    Some(site)
}

/// A carried prototype layout to emit: its descriptor, keys and
/// [[Prototype]] (`None`: `%Object.prototype%`; `Some(None)`: null;
/// `Some(Some(g))`: the `PerryProtoFamily` `g`).
pub(crate) struct ImageDesc {
    pub desc_global: String,
    pub entries: Vec<ImageEntry>,
    pub proto: Option<Option<String>>,
}

/// The prototype records, family descriptors and carried prototype layouts
/// to emit: every followed prototype an `Object.create` names, each layout
/// lowered code used, and the carried layout of each such prototype the
/// compiler can name exactly.
pub(crate) fn emission() -> (Vec<String>, Vec<Site>, Vec<ImageDesc>) {
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        let protos = m
            .bound
            .iter()
            .map(|id| format!("perry_protofam_{}_{id}", m.prefix))
            .collect();
        let images = m
            .bound
            .iter()
            .filter(|_| m.checked)
            .filter_map(|id| {
                let b = &m.borns[id];
                let entries = b.image.clone()?;
                let proto = match b.proto {
                    Link::Object => None,
                    Link::Null => Some(None),
                    Link::Binding(q) if m.bound.contains(&q) => {
                        Some(Some(format!("perry_protofam_{}_{q}", m.prefix)))
                    }
                    Link::Binding(_) => return None,
                };
                Some(ImageDesc {
                    desc_global: image_global(&m.prefix, *id),
                    entries,
                    proto,
                })
            })
            .collect();
        let sites = if crate::layout_ordinals::collecting() {
            m.candidates.values().cloned().collect()
        } else {
            m.used.values().cloned().collect()
        };
        (protos, sites, images)
    })
}

/// `Object.create(P)` for a binding's family: born on its first state (every
/// slot a hole) when `P` is the family's prototype, else the ordinary birth.
pub(crate) fn lower_create(ctx: &mut FnCtx<'_>, family: &Create, expr: &Expr) -> Result<String> {
    let Expr::ObjectCreate(proto, None) = expr else {
        anyhow::bail!("create family birth is not Object.create(P)");
    };
    let proto_box = crate::expr::lower_expr(ctx, proto)?;
    Ok(ctx.block().call(
        DOUBLE,
        "perry_create_family",
        &[
            (PTR, &format!("@{}", family.states[0])),
            (DOUBLE, &proto_box),
        ],
    ))
}

/// `PERRY_CHECK_CONTRACTS=1`: assert what a family store relied on.
fn emit_contract(ctx: &mut FnCtx<'_>, desc_global: &str, object: &str) {
    ctx.block().call_void(
        "js_create_family_contract_check",
        &[(PTR, &format!("@{desc_global}")), (DOUBLE, object)],
    );
}

/// The registered id of a carried family descriptor, loaded once per
/// function (written once at startup, before any user code).
fn layout_id(ctx: &mut FnCtx<'_>, desc_global: &str) -> String {
    let slot = ctx
        .func
        .entry_init_load_global(&super::layout_id_global(desc_global), I32);
    ctx.block().load(I32, &slot)
}

/// GcHeader `_reserved` bits that refuse a key add: frozen, sealed, not
/// extensible (runtime `gc::OBJ_FLAG_FROZEN | OBJ_FLAG_SEALED |
/// OBJ_FLAG_NO_EXTEND`).
const RECEIVER_INTEGRITY_BITS: &str = "7";

/// The checked proof of a family add (decision 150): `i1`, every prototype
/// of the chain still on its carried layout (its module global's layout
/// word equals the layout's id, which also refuses a shadowed word) and
/// `%Object.prototype%` unwritten. Emitted only after the receiver proved
/// its state, so each binding holds its prototype (the receiver was born of
/// it, after the chain's bindings were initialized).
fn emit_chain_proof(ctx: &mut FnCtx<'_>, family: &Create) -> String {
    let mut ok = "true".to_string();
    for (h, desc) in &family.chain {
        let Some(global) = ctx.module_globals.get(h).cloned() else {
            return "false".to_string();
        };
        let id = layout_id(ctx, desc);
        let blk = ctx.block();
        let value = blk.load(DOUBLE, &format!("@{global}"));
        let bits = blk.bitcast_double_to_i64(&value);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let ptr = blk.inttoptr(I64, &handle);
        let word_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
        let word = blk.load(I32, &word_ptr);
        let registered = blk.icmp_ne(I32, &id, "0");
        let same = blk.icmp_eq(I32, &word, &id);
        let held = blk.and(crate::types::I1, &registered, &same);
        ok = blk.and(crate::types::I1, &ok, &held);
    }
    if family.object_root {
        let patched = crate::builtin_proto::emit_patched_test(
            ctx,
            crate::builtin_proto::BuiltinProto::Object,
        );
        let blk = ctx.block();
        let unpatched = blk.xor(crate::types::I1, &patched, "true");
        ok = blk.and(crate::types::I1, &ok, &unpatched);
    }
    ok
}

/// `a.k = v` for a binding family's add of `keys[index]`. A receiver in the
/// state before it (the add) or in the state holding it (an overwrite of its
/// own writable data), with no meta record (no overflow, no elements, no
/// recorded [[Prototype]]: its prototype is the family's) and not frozen,
/// sealed or made non-extensible, stores the slot and the holding state's
/// layout word, when the census proved the add free (no prototype is read)
/// or the chain's checked proof holds. Any other receiver takes the
/// ordinary `[[Set]]`. Returns the stored value.
pub(crate) fn lower_add(
    ctx: &mut FnCtx<'_>,
    add: &Add,
    object: &Expr,
    value: &Expr,
    strict: bool,
) -> Result<String> {
    let family = &add.family;
    let slot = add.index;
    let barrier_needed = !crate::expr::expr_produces_non_pointer_bits_by_construction(ctx, value);
    let addref_needed = crate::expr::class_field_store_needs_string_addref(ctx, value);
    rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
        let (recv_box, val_double) = (vals[0].clone(), vals[1].clone());
        let before = layout_id(ctx, &family.states[slot]);
        let after = layout_id(ctx, &family.states[slot + 1]);
        let check_idx = ctx.new_block("create_add.layout");
        let chain_idx = (!family.chain.is_empty()).then(|| ctx.new_block("create_add.chain"));
        let fast_idx = ctx.new_block("create_add.slot");
        let slow_idx = ctx.new_block("create_add.generic");
        let merge_idx = ctx.new_block("create_add.merge");
        let check_label = ctx.block_label(check_idx);
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        let obj_bits = ctx.block().bitcast_double_to_i64(&recv_box);
        let recv = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), &obj_bits);
        ctx.block()
            .cond_br(&recv.is_object_pointer, &check_label, &slow_label);

        ctx.current_block = check_idx;
        let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, add.form());
        let pointer = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
            4
        } else {
            8
        };
        let receiver_done = chain_idx.map_or(fast_label.clone(), |c| ctx.block_label(c));
        let (obj_handle, obj_ptr) = {
            let blk = ctx.block();
            let obj_handle = crate::expr::receiver_range::emit_handle(blk, &recv.biased);
            let obj_ptr = blk.inttoptr(I64, &obj_handle);
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            let word = blk.load(I32, &word_ptr);
            let registered = blk.icmp_ne(I32, &after, "0");
            let adding = blk.icmp_eq(I32, &word, &before);
            let overwrite = blk.icmp_eq(I32, &word, &after);
            let either = blk.or(crate::types::I1, &adding, &overwrite);
            let meta_ptr = blk.gep(I8, &obj_ptr, &[(I64, &(header - pointer).to_string())]);
            let meta = blk.load(PTR, &meta_ptr);
            let no_meta = blk.icmp_eq(PTR, &meta, "null");
            let gc_ptr = blk.gep(
                I8,
                &obj_ptr,
                &[(I64, &(2 - perry_abi::GC_HEADER_SIZE as i64).to_string())],
            );
            let gc_bits = blk.load(crate::types::I16, &gc_ptr);
            let restricted = blk.and(crate::types::I16, &gc_bits, RECEIVER_INTEGRITY_BITS);
            let extensible = blk.icmp_eq(crate::types::I16, &restricted, "0");
            let ok = blk.and(crate::types::I1, &registered, &either);
            let ok = blk.and(crate::types::I1, &ok, &no_meta);
            let ok = blk.and(crate::types::I1, &ok, &extensible);
            blk.cond_br(&ok, &receiver_done, &slow_label);
            (obj_handle, obj_ptr)
        };

        if let Some(chain_idx) = chain_idx {
            ctx.current_block = chain_idx;
            let held = emit_chain_proof(ctx, family);
            ctx.block().cond_br(&held, &fast_label, &slow_label);
        }

        ctx.current_block = fast_idx;
        let (slot_ptr, slot_addr) = {
            let blk = ctx.block();
            let offset = (header + 8 * slot as u64).to_string();
            let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &offset)]);
            let slot_addr = blk.ptrtoint(&slot_ptr, I64);
            (slot_ptr, slot_addr)
        };
        crate::expr::emit_jsvalue_slot_store_pointer_tested(
            ctx,
            &slot_ptr,
            &val_double,
            &obj_handle,
            addref_needed,
            &obj_bits,
            &slot_addr,
            barrier_needed,
            "family_set",
        );
        {
            let blk = ctx.block();
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            // GC_STORE_AUDIT(POINTER_FREE): the layout word is a scalar.
            blk.store(I32, &after, &word_ptr);
        }
        if crate::codegen::helpers::check_contracts() {
            emit_contract(ctx, &family.states[slot + 1], &recv_box);
        }
        ctx.block().br(&merge_label);
        let fast_end = ctx.block_label(ctx.current_block);

        ctx.current_block = slow_idx;
        let key_idx = ctx.strings.intern(&family.keys[slot]);
        let handle = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let key_box = ctx.block().load(DOUBLE, &handle);
        let slow_val = ctx.block().call(
            DOUBLE,
            "js_put_value_set",
            &[
                (DOUBLE, &recv_box),
                (DOUBLE, &key_box),
                (DOUBLE, &val_double),
                (DOUBLE, &recv_box),
                (I32, if strict { "1" } else { "0" }),
            ],
        );
        ctx.block().br(&merge_label);
        let slow_end = ctx.block_label(ctx.current_block);

        ctx.current_block = merge_idx;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&val_double, &fast_end), (&slow_val, &slow_end)]))
    })
}

/// `P`'s birth: after the top-level binding `id` is initialized, give the
/// object the family's prototype serial, and, when the compiler derived its
/// carried layout, let the runtime bear it on that layout (the binding then
/// holds the object born there; `stmt/mod.rs`).
pub(crate) fn lower_binding(ctx: &mut FnCtx<'_>, id: u32) -> Result<()> {
    let Some((family, image, locals)) = MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        m.bound.contains(&id).then(|| {
            let image = m.borns[&id].image.as_ref().filter(|_| m.checked);
            let locals: Vec<u32> = image
                .into_iter()
                .flatten()
                .filter_map(|e| match e.key {
                    ImageKey::Local(k) => Some(k),
                    _ => None,
                })
                .collect();
            (
                format!("perry_protofam_{}_{id}", m.prefix),
                image.map(|_| image_global(&m.prefix, id)),
                locals,
            )
        })
    }) else {
        return Ok(());
    };
    // Only a module global can take the image (an add reads it there).
    let global = ctx.module_globals.get(&id).cloned();
    let image = image.filter(|_| global.is_some());
    // The image's module-local symbol keys: their bindings' values, in key
    // order (each binding is initialized: `P`'s initializer read it).
    let mut symbols = Vec::new();
    if image.is_some() {
        for k in &locals {
            symbols.push(crate::expr::lower_expr(ctx, &Expr::LocalGet(*k))?);
        }
    }
    let read = Expr::LocalGet(id);
    let value = crate::expr::lower_expr(ctx, &read)?;
    let image_arg = image
        .as_ref()
        .map_or("null".to_string(), |d| format!("@{d}"));
    let symbols_arg = if symbols.is_empty() {
        "null".to_string()
    } else {
        // The call copies them before it allocates. In the entry block (#167).
        let buf = ctx.func.alloca_entry_array(DOUBLE, symbols.len());
        for (i, v) in symbols.iter().enumerate() {
            let blk = ctx.block();
            let at = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
            blk.store(DOUBLE, v, &at);
        }
        buf
    };
    let born = ctx.block().call(
        DOUBLE,
        "perry_bind_family_prototype",
        &[
            (PTR, &format!("@{family}")),
            (DOUBLE, &value),
            (PTR, &image_arg),
            (PTR, &symbols_arg),
            (I32, &symbols.len().to_string()),
        ],
    );
    if let (Some(_), Some(global)) = (image, global) {
        crate::expr::emit_gated_root_nanbox_store(ctx, &born, &format!("@{global}"));
    }
    Ok(())
}

/// The most keys one binding's family holds.
const MAX_CREATE_KEYS: usize = 16;

/// The prototype bindings of `p`'s chain (`p` first), when every link up to
/// `%Object.prototype%` or null is a binding the compiler follows.
fn chain_of(p: u32, borns: &HashMap<u32, Born>) -> Option<Vec<u32>> {
    let mut chain = vec![p];
    loop {
        match borns.get(chain.last()?)?.proto {
            Link::Object | Link::Null => return Some(chain),
            Link::Binding(q) => {
                if chain.contains(&q) || chain.len() == 8 {
                    return None;
                }
                chain.push(q);
            }
        }
    }
}

/// The keys of `b`'s chain that are, or may be, accessors or non-writable.
fn chain_blocks(b: &Born, borns: &HashMap<u32, Born>) -> BTreeSet<String> {
    let mut blocks = b.blocks.clone();
    let mut link = b.proto;
    for _ in 0..8 {
        let Link::Binding(q) = link else { break };
        let Some(next) = borns.get(&q) else { break };
        blocks.extend(next.blocks.iter().cloned());
        link = next.proto;
    }
    blocks
}

/// Add or redefine `key` in `image` (its position is kept on a redefine).
fn upsert(image: &mut Vec<ImageEntry>, key: ImageKey, attrs: u8) {
    match image.iter_mut().find(|e| e.key == key) {
        Some(e) => e.attrs = attrs,
        None => image.push(ImageEntry { key, attrs }),
    }
}

/// Copy `source`'s own enumerable keys onto `target` in their
/// [[OwnPropertyKeys]] order (strings, then symbols): `Object.assign`'s
/// `[[Set]]`s (`assign`: an existing key keeps its attributes, and must be
/// writable data) or a spread's CreateDataProperty (a plain data property).
/// An accessor of the source yields its value: data on the target.
fn copy_keys(target: &mut Option<Vec<ImageEntry>>, source: Option<Vec<ImageEntry>>, assign: bool) {
    let (Some(t), Some(source)) = (target.as_mut(), source) else {
        *target = None;
        return;
    };
    let enumerable = |e: &&ImageEntry| e.attrs & NON_ENUMERABLE == 0;
    let ordered = source
        .iter()
        .filter(enumerable)
        .filter(|e| !e.key.is_symbol())
        .chain(
            source
                .iter()
                .filter(enumerable)
                .filter(|e| e.key.is_symbol()),
        );
    for e in ordered {
        match t.iter_mut().find(|x| x.key == e.key) {
            Some(x) if assign => {
                if !plain_data(x.attrs) {
                    *target = None;
                    return;
                }
            }
            Some(x) => x.attrs = 0,
            None => t.push(ImageEntry {
                key: e.key.clone(),
                attrs: 0,
            }),
        }
    }
}

/// [`Born`] of a prototype binding's initializer `e`, when the compiler can
/// follow it: a literal, `Object.create(Q)` with `Q` null or an earlier such
/// binding, `Object.assign(T, literal…)`, `Object.defineProperty(T, "k",
/// literal)`. No user code that could reach the object runs before the
/// binding holds it: a literal's keys and values never see it, a source's
/// getters see the source, and the binding is in its TDZ until then.
fn born(a: &Analysis<'_>, e: &Expr) -> Option<Born> {
    match e {
        Expr::ObjectCreate(proto, None) => {
            let proto = match proto.as_ref() {
                Expr::Null => Link::Null,
                Expr::LocalGet(q) if a.borns.contains_key(q) => Link::Binding(*q),
                _ => return None,
            };
            Some(Born {
                proto,
                image: Some(Vec::new()),
                ..Born::default()
            })
        }
        Expr::ObjectAssign { target, sources } => {
            let mut t = born(a, target)?;
            // Each source key is a `[[Set]]` on the target: refused where a
            // setter could run (user code with the object) or a key could
            // relink it. The source's own accessors yield data.
            let blocks = chain_blocks(&t, &a.borns);
            for src in sources {
                let lit = literal(a, src)?;
                if (lit.open && !blocks.is_empty())
                    || lit
                        .names
                        .iter()
                        .any(|k| k == "__proto__" || blocks.contains(k))
                {
                    return None;
                }
                t.dynamic |= lit.open;
                copy_keys(&mut t.image, lit.image, true);
            }
            Some(t)
        }
        Expr::ObjectDefineProperty(target, key, desc) => {
            let Expr::String(k) = key.as_ref() else {
                return None;
            };
            literal(a, desc)?;
            let mut t = born(a, target)?;
            // Defaults make a new key non-writable; an accessor intercepts.
            t.blocks.insert(k.clone());
            let key = ImageKey::Str(k.clone());
            match (t.image.as_mut(), descriptor_attrs(a, desc)) {
                (Some(img), Some(attrs)) if !img.iter().any(|x| x.key == key) => {
                    img.push(ImageEntry { key, attrs })
                }
                _ => t.image = None,
            }
            Some(t)
        }
        _ => literal(a, e).map(|l| Born {
            blocks: l.blocks,
            image: l.image,
            ..Born::default()
        }),
    }
}

/// The attribute entry `Object.defineProperty(T, k, desc)` gives a new key
/// `k`, when `desc` is a literal whose fields the compiler reads: getter and
/// setter presence, and literal booleans (absent ones default to false).
fn descriptor_attrs(a: &Analysis<'_>, desc: &Expr) -> Option<u8> {
    let fields: Vec<(&str, &Expr)> = match desc {
        Expr::New {
            class_name, args, ..
        } => {
            let c = a
                .classes
                .iter()
                .find(|c| &c.name == class_name)
                .filter(|c| c.is_literal_shape())?;
            if c.fields.len() != args.len() || c.fields.iter().any(|f| f.key_expr.is_some()) {
                return None;
            }
            c.fields.iter().map(|f| f.name.as_str()).zip(args).collect()
        }
        Expr::Object(props) => props.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        _ => return None,
    };
    let function = |v: &Expr| matches!(v, Expr::Closure { .. } | Expr::FuncRef(_));
    let mut attrs = NON_WRITABLE | NON_ENUMERABLE | NON_CONFIGURABLE;
    let mut accessor = 0u8;
    let mut data = false;
    for (name, value) in fields {
        let flag = match value {
            Expr::Bool(b) => Some(*b),
            _ => None,
        };
        match name {
            "get" if function(value) => accessor |= ACCESSOR | HAS_GET,
            "set" if function(value) => accessor |= ACCESSOR | HAS_SET,
            "value" => data = true,
            "writable" => {
                data = true;
                if flag? {
                    attrs &= !NON_WRITABLE;
                }
            }
            "enumerable" => {
                if flag? {
                    attrs &= !NON_ENUMERABLE;
                }
            }
            "configurable" => {
                if flag? {
                    attrs &= !NON_CONFIGURABLE;
                }
            }
            _ => return None,
        }
    }
    if accessor != 0 {
        if data {
            return None;
        }
        // An accessor has no [[Writable]].
        return Some(accessor | (attrs & !NON_WRITABLE));
    }
    Some(attrs)
}

/// A literal's facts: the keys that are or may be accessors, the string
/// keys it names, whether it may hold keys it does not name, and its own
/// keys in order when the compiler can name every one (its [[Prototype]] is
/// `%Object.prototype%`).
struct Lit {
    blocks: BTreeSet<String>,
    names: BTreeSet<String>,
    open: bool,
    image: Option<Vec<ImageEntry>>,
}

fn literal(a: &Analysis<'_>, e: &Expr) -> Option<Lit> {
    let mut lit = Lit {
        blocks: BTreeSet::new(),
        names: BTreeSet::new(),
        open: false,
        image: Some(Vec::new()),
    };
    let name = |lit: &mut Lit, k: &str| {
        lit.names.insert(k.to_string());
        if let Some(img) = lit.image.as_mut() {
            upsert(img, ImageKey::Str(k.to_string()), 0);
        }
    };
    match e {
        Expr::New { class_name, .. } => {
            let c = a
                .classes
                .iter()
                .find(|c| &c.name == class_name)
                .filter(|c| c.is_literal_shape())?;
            for f in &c.fields {
                if f.key_expr.is_some() {
                    lit.open = true;
                    lit.image = None;
                } else {
                    name(&mut lit, &f.name);
                }
            }
        }
        Expr::Object(props) => {
            for (k, _) in props {
                name(&mut lit, k);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (k, v) in parts {
                match k {
                    Some(k) => name(&mut lit, k),
                    None => {
                        lit.open = true;
                        copy_keys(&mut lit.image, a.source_image(v), false);
                    }
                }
            }
        }
        Expr::Call { callee, args, .. } => {
            literal_iife(a, callee, args, &mut lit)?;
        }
        _ => return None,
    }
    // `__proto__: v` in a literal sets the [[Prototype]].
    if lit.names.contains("__proto__") {
        return None;
    }
    Some(lit)
}

/// The object-literal IIFE codegen's lowering of a literal with computed
/// keys or spreads makes: `(o => { define(o, "k", v); set_computed(o, key,
/// v); spread(o, src); method(o, key, f); accessor(o, key, get, set); …;
/// return o })({})`. Every key the compiler can name goes to `names`, an
/// accessor's to `blocks` too, and each key in order to the image. `None`
/// for anything else (an unnamed accessor, `o` escaping).
fn literal_iife(a: &Analysis<'_>, callee: &Expr, args: &[Expr], lit: &mut Lit) -> Option<()> {
    let Expr::Closure { params, body, .. } = callee else {
        return None;
    };
    // The object starts empty: `{}`, or the old-generation birth of a
    // module-singleton literal (`js_object_literal_prototype_birth`).
    let empty = |e: &Expr| match e {
        Expr::Object(p) => p.is_empty(),
        Expr::Call { callee, args, .. } => {
            args.is_empty()
                && matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. }
                    if name == "js_object_literal_prototype_birth")
        }
        _ => false,
    };
    if params.len() != 1 || args.len() != 1 || !empty(&args[0]) {
        return None;
    }
    let o = params[0].id;
    let mut keys: HashMap<u32, Option<KeyConst>> = HashMap::new();
    let key_of = |e: &Expr, keys: &HashMap<u32, Option<KeyConst>>| -> Option<KeyConst> {
        match e {
            Expr::LocalGet(x) if keys.contains_key(x) => keys[x].clone(),
            other => a.key_const(other),
        }
    };
    let data = |lit: &mut Lit, k: Option<KeyConst>| match k {
        Some(KeyConst::Str(k)) => {
            lit.names.insert(k.clone());
            if let Some(img) = lit.image.as_mut() {
                upsert(img, ImageKey::Str(k), 0);
            }
        }
        Some(KeyConst::Symbol(Some(k))) => {
            if let Some(img) = lit.image.as_mut() {
                upsert(img, k, 0);
            }
        }
        Some(KeyConst::Symbol(None)) => lit.image = None,
        None => {
            lit.open = true;
            lit.image = None;
        }
    };
    for (i, stmt) in body.iter().enumerate() {
        match stmt {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(init),
                ..
            } if !mentions(init, o) => {
                let key = match init {
                    Expr::Call { callee, args, .. }
                        if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. }
                            if name == "js_object_literal_to_property_key")
                            && args.len() == 1 =>
                    {
                        key_of(&args[0], &keys)
                    }
                    other => key_of(other, &keys),
                };
                keys.insert(*id, key);
            }
            Stmt::Expr(Expr::Call { callee, args, .. }) => {
                let Expr::ExternFuncRef { name, .. } = callee.as_ref() else {
                    return None;
                };
                let on_o = matches!(args.first(), Some(Expr::LocalGet(x)) if *x == o);
                if args.iter().skip(usize::from(on_o)).any(|a| mentions(a, o)) {
                    return None;
                }
                match (name.as_str(), on_o) {
                    ("js_object_literal_define", true) if args.len() == 3 => match &args[1] {
                        Expr::String(k) if k != "__proto__" => {
                            data(lit, Some(KeyConst::Str(k.clone())))
                        }
                        _ => return None,
                    },
                    (
                        "js_object_literal_set_computed"
                        | "js_object_set_method_by_name"
                        | "js_object_set_property_key_method",
                        true,
                    ) if args.len() == 3 => data(lit, key_of(&args[1], &keys)),
                    ("js_object_define_accessor", true) if args.len() == 4 => {
                        let half = |v: &Expr| !matches!(v, Expr::Undefined | Expr::Null);
                        let bits = ACCESSOR
                            | if half(&args[2]) { HAS_GET } else { 0 }
                            | if half(&args[3]) { HAS_SET } else { 0 };
                        let key = match key_of(&args[1], &keys)? {
                            KeyConst::Str(k) => {
                                lit.names.insert(k.clone());
                                lit.blocks.insert(k.clone());
                                Some(ImageKey::Str(k))
                            }
                            KeyConst::Symbol(k) => k,
                        };
                        match (key, lit.image.as_mut()) {
                            (Some(k), Some(img)) => {
                                // `get k` and `set k` make one accessor.
                                let merged = img
                                    .iter()
                                    .find(|e| e.key == k && e.attrs & ACCESSOR != 0)
                                    .map_or(bits, |e| e.attrs | bits);
                                upsert(img, k, merged);
                            }
                            _ => lit.image = None,
                        }
                    }
                    ("js_object_literal_spread", true) if args.len() == 2 => {
                        lit.open = true;
                        let source = a.source_image(&args[1]);
                        copy_keys(&mut lit.image, source, false);
                    }
                    ("js_object_literal_infer_computed_function_name", false) => {}
                    _ => return None,
                }
            }
            Stmt::Return(Some(Expr::LocalGet(x))) if *x == o && i + 1 == body.len() => {
                return Some(());
            }
            _ => return None,
        }
    }
    None
}

/// Does `e` read, write or capture the local `id`?
fn mentions(e: &Expr, id: u32) -> bool {
    match e {
        Expr::LocalGet(x) | Expr::LocalSet(x, _) if *x == id => return true,
        Expr::Closure { captures, .. } if captures.contains(&id) => return true,
        _ => {}
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(e, &mut |c| found = found || mentions(c, id));
    found
}

/// `a.k = v` (either spelling) with `a` a local: `(a, k)`.
fn static_store(e: &Expr) -> Option<(u32, &str)> {
    match e {
        Expr::PropertySet {
            object, property, ..
        } => match object.as_ref() {
            Expr::LocalGet(a) => Some((*a, property.as_str())),
            _ => None,
        },
        Expr::PutValueSet {
            target,
            key,
            receiver,
            ..
        } => match (target.as_ref(), key.as_ref(), receiver.as_ref()) {
            (Expr::LocalGet(a), Expr::String(k), Expr::LocalGet(r)) if a == r => {
                Some((*a, k.as_str()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// `Object.assign(Object.create(P), { k1: e1, … })` for a family `site`.
///
/// Evaluation order is the operation's: `P`, the object's creation (which
/// throws for a `P` that is not an object or null), then `e1…en`, then the
/// adds. The family's object is born with every slot a hole and is not
/// observable before its stores; any other object gets the ordinary
/// `[[Set]]`s (`js_put_value_set`, strict, as `Object.assign` throws).
pub(crate) fn lower_assign(ctx: &mut FnCtx<'_>, site: &Site, expr: &Expr) -> Result<String> {
    let Expr::ObjectAssign { target, sources } = expr else {
        anyhow::bail!("create family site is not Object.assign");
    };
    let Expr::ObjectCreate(proto, None) = target.as_ref() else {
        anyhow::bail!("create family target is not Object.create(P)");
    };
    let Expr::New { args, .. } = &sources[0] else {
        anyhow::bail!("create family source is not a literal");
    };
    let n = args.len();
    rooting::with_rooted_group(ctx, n + 1, |ctx, group| {
        let proto_box = crate::expr::lower_expr(ctx, proto)?;
        let born = ctx.block().call(
            DOUBLE,
            "perry_create_family",
            &[
                (PTR, &format!("@{}", site.desc_global)),
                (DOUBLE, &proto_box),
            ],
        );
        let object = group.adopt_emitted(ctx, Repr::Boxed, &born, true);
        let mut operands = Vec::with_capacity(n);
        for arg in args {
            operands.push(group.lower(ctx, arg, true)?);
        }

        // The layout word tells the two births apart: the ordinary one is
        // keyless, the family's carries `k1…kn`.
        let object_box = group.reread_emitted(ctx, object);
        let id_global = super::layout_id_global(&site.desc_global);
        let id_slot = ctx.func.entry_init_load_global(&id_global, I32);
        let (obj_bits, obj_handle, obj_ptr, is_family) = {
            let blk = ctx.block();
            let id = blk.load(I32, &id_slot);
            let obj_bits = blk.bitcast_double_to_i64(&object_box);
            let obj_handle = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
            let obj_ptr = blk.inttoptr(I64, &obj_handle);
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            let word = blk.load(I32, &word_ptr);
            let registered = blk.icmp_ne(I32, &id, "0");
            let same = blk.icmp_eq(I32, &word, &id);
            let is_family = blk.and(crate::types::I1, &registered, &same);
            (obj_bits, obj_handle, obj_ptr, is_family)
        };
        let family_idx = ctx.new_block("create_family.slots");
        let generic_idx = ctx.new_block("create_family.set");
        let merge_idx = ctx.new_block("create_family.merge");
        let family_label = ctx.block_label(family_idx);
        let generic_label = ctx.block_label(generic_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block()
            .cond_br(&is_family, &family_label, &generic_label);

        // The family's slots: nothing between the re-reads and the stores
        // collects, so the object and the values are current.
        ctx.current_block = family_idx;
        let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, site.form());
        for (slot, arg) in args.iter().enumerate() {
            let value = group.reread(ctx, operands[slot])?;
            let barrier_needed =
                !crate::expr::expr_produces_non_pointer_bits_by_construction(ctx, arg);
            let addref_needed = crate::expr::class_field_store_needs_string_addref(ctx, arg);
            let offset = (header + 8 * slot as u64).to_string();
            let (slot_ptr, slot_addr) = {
                let blk = ctx.block();
                let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &offset)]);
                let slot_addr = blk.ptrtoint(&slot_ptr, I64);
                (slot_ptr, slot_addr)
            };
            crate::expr::emit_jsvalue_slot_store_pointer_tested(
                ctx,
                &slot_ptr,
                &value,
                &obj_handle,
                addref_needed,
                &obj_bits,
                &slot_addr,
                barrier_needed,
                "family_set",
            );
        }
        if crate::codegen::helpers::check_contracts() {
            emit_contract(ctx, &site.desc_global, &object_box);
        }
        ctx.block().br(&merge_label);
        let family_end = ctx.block_label(ctx.current_block);

        // Any other object: the ordinary adds, in order. Each can collect
        // (a setter runs user code), so every operand is re-read per add.
        ctx.current_block = generic_idx;
        for (slot, key) in site.keys.iter().enumerate() {
            let key_idx = ctx.strings.intern(key);
            let handle = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let key_box = ctx.block().load(DOUBLE, &handle);
            let target_box = group.reread_emitted(ctx, object);
            let value = group.reread(ctx, operands[slot])?;
            ctx.block().call(
                DOUBLE,
                "js_put_value_set",
                &[
                    (DOUBLE, &target_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &value),
                    (DOUBLE, &target_box),
                    (I32, "1"),
                ],
            );
        }
        let generic_box = group.reread_emitted(ctx, object);
        ctx.block().br(&merge_label);
        let generic_end = ctx.block_label(ctx.current_block);

        ctx.current_block = merge_idx;
        Ok(ctx.block().phi(
            DOUBLE,
            &[(&object_box, &family_end), (&generic_box, &generic_end)],
        ))
    })
}

mod header_form;
