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

mod models;
mod site;
mod emission;

pub(crate) use emission::{
    after_stmt, before_stmt, emission, emit_holder_load, lower_add, lower_assign, lower_binding,
    lower_create, lower_holder_read, ImageDesc,
};

pub(crate) use site::{add_site, create_site, holder_read, site};
use site::literal_add_site;

pub(crate) use models::{Add, Create, CreateState, HolderRead, Site};
pub use models::{ExportFact, ImageEntry, ImageKey, ProtoExports};
use models::{exportable, plain_data};
use models::{ACCESSOR, HAS_GET, HAS_SET, NON_CONFIGURABLE, NON_ENUMERABLE, NON_WRITABLE};

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


