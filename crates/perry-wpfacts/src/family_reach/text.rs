//! L3: what the program text names. A second, syntactic predicate for the
//! same fact; it only ever adds `live`, and it is never the reason for a
//! `never`.
//!
//! Three parts, each over EVERY body of every module (walked or not), so
//! that the fact does not read which bodies can run:
//!
//! - **names**: a lowered intrinsic node ([`super::nodes`]), a module
//!   specifier, a global of a family, and a global holder of core used
//!   other than as the receiver of a static member (`process` as a value:
//!   everything below it and what those entries declare born; `String.raw`:
//!   that entry only). The global object itself as a value, or under a key
//!   that is not a constant, is every family. A constant computed key
//!   (`globalThis["Uint8" + "Array"]`) is a static key; listing the global
//!   object's keys is no observation.
//! - **static member keys**: a string the text holds that is the key of a
//!   prototype member of a family its holder is not of (`normalize` on
//!   `String.prototype`, `toLocaleString` on `Date.prototype`), wherever it
//!   stands: a prototype's values exist without the holder being named. (A
//!   static member needs its holder named: the names part.)
//! - **computed keys**: only in a body generation does not walk (in a walked
//!   body the solve has the key's strings and the receivers): any member of
//!   a family on a core prototype.
//!
//! No family has an entry on a core prototype that is a byte cell or
//! declares one born (`byte_cells_are_on_no_core_prototype`), so for the
//! typed-array family the third part is empty over every body.

use super::nodes::{self, Node};
use super::{families, index, FamilySet};
use crate::gen::{Gen, PendingWalk};
use crate::pts::FxHashSet;
use perry_hir::{Expr, Module};

type Mark<'f> = dyn FnMut(FamilySet, &dyn Fn() -> String) + 'f;

fn addr(e: &Expr) -> usize {
    e as *const Expr as usize
}

/// The constant string `e` is, through `+` of constants.
fn const_str(e: &Expr) -> Option<String> {
    match e {
        Expr::String(s) => Some(s.clone()),
        Expr::Binary { left, right, op } if matches!(op, perry_hir::BinaryOp::Add) => {
            Some(const_str(left)? + &const_str(right)?)
        }
        _ => None,
    }
}

/// `(object, key)` of a member access whose key is a constant.
fn static_member(e: &Expr) -> Option<(&Expr, std::borrow::Cow<'_, str>)> {
    match e {
        Expr::PropertyGet {
            object, property, ..
        }
        | Expr::PropertySet {
            object, property, ..
        }
        | Expr::PropertyUpdate {
            object, property, ..
        } => Some((object, property.as_str().into())),
        Expr::IndexGet { object, index, .. }
        | Expr::IndexSet { object, index, .. }
        | Expr::IndexUpdate { object, index, .. } => Some((object, const_str(index)?.into())),
        _ => None,
    }
}

/// The receiver of a member access under a key that is a value.
fn computed_receiver(e: &Expr) -> Option<&Expr> {
    match e {
        Expr::IndexGet { object, index, .. }
        | Expr::IndexSet { object, index, .. }
        | Expr::IndexUpdate { object, index, .. }
            if const_str(index).is_none() =>
        {
            Some(object)
        }
        _ => None,
    }
}

struct Scan<'m, 's, 'f> {
    module: &'m Module,
    mark: &'s mut Mark<'f>,
    interp_linked: bool,
    /// Global-object expressions a parent reads a static member of, or
    /// lists the keys of.
    receivers: FxHashSet<usize>,
    /// Named-global expressions a parent reads a static member of.
    member_receivers: FxHashSet<usize>,
    /// Named-global expressions a parent calls, constructs or tests.
    used_in_place: FxHashSet<usize>,
}

impl Scan<'_, '_, '_> {
    /// The HIR sentinel of the global scope (`gen_expr`: id 0 with no
    /// declared global of that id).
    fn sentinel(&self, e: &Expr) -> bool {
        matches!(e, Expr::GlobalGet(0)) && !self.module.globals.iter().any(|g| g.id == 0)
    }

    /// `e` evaluates to the global object.
    fn global_object(&self, e: &Expr) -> bool {
        match e {
            Expr::GlobalThisExpr => true,
            Expr::PropertyGet {
                object, property, ..
            } => {
                matches!(property.as_str(), "globalThis" | "global")
                    && (self.sentinel(object) || self.global_object(object))
            }
            _ => false,
        }
    }

    /// The global `e` names (`Uint8Array`, `globalThis.process`).
    fn named_global<'e>(&self, e: &'e Expr) -> Option<std::borrow::Cow<'e, str>> {
        let (object, key) = static_member(e)?;
        (matches!(e, Expr::PropertyGet { .. } | Expr::IndexGet { .. })
            && (self.sentinel(object) || self.global_object(object))
            && !matches!(&*key, "globalThis" | "global"))
        .then_some(key)
    }

    fn all(&mut self, why: &dyn Fn() -> String) {
        (self.mark)(families::all_families(), why);
    }

    /// The module of a `NativeMethodCall` / `NativeModuleRef`. Lowering also
    /// uses the field for receivers that are no module: its own helpers
    /// (`__perry_runtime`: iteration records, destructuring) and a few
    /// built-in receivers by kind.
    fn module_spec(&mut self, spec: &str) {
        let ix = index();
        match spec {
            s if s.starts_with("__perry_") => return,
            "array" => return,
            "bigint" => {
                return (self.mark)(families::family_bit("bigint"), &|| {
                    "L3:a BigInt method".into()
                })
            }
            "fetch" | "Headers" | "readable_stream" | "readable_stream_reader" => {
                let set = ["fetch", "Headers", "ReadableStream"]
                    .iter()
                    .fold(0, |set, name| {
                        set | families::entry_families(name).unwrap_or(0) | ix.root(name)
                    });
                return (self.mark)(set, &|| format!("L3:a {spec} method"));
            }
            _ => {}
        }
        match families::module_families(spec) {
            Some(set) => {
                let root = format!("node:{}", spec.strip_prefix("node:").unwrap_or(spec));
                let set = set | index().root(&root);
                (self.mark)(set, &|| format!("L3:module {spec}"));
            }
            None => self.all(&|| format!("L3:module {spec} is not classified")),
        }
    }

    /// Global `name` used as a value: everything below it.
    fn global_value(&mut self, name: &str) {
        if matches!(name, "Function" | "eval") {
            if self.interp_linked {
                self.all(&|| format!("L3:{name} is named and the interpreter is linked"));
            }
            return;
        }
        let ix = index();
        if ix.entry(name).is_some() || ix.roots.contains_key(name) {
            (self.mark)(ix.root(name), &|| format!("L3:{name} is used as a value"));
        } else if let Some(&(set, entry)) = ix.namespace_keys.get(name) {
            // Lowering can keep a namespace member read as a read off the
            // global sentinel (`Math.floor` as a value).
            (self.mark)(set, &|| format!("L3:{entry} (a namespace member by key)"));
        }
    }

    /// Global `name` called, constructed or tested in place: that entry.
    fn global_in_place(&mut self, name: &str) {
        if matches!(name, "Function" | "eval") {
            return self.global_value(name);
        }
        let ix = index();
        match ix.entry(name) {
            Some(set) => (self.mark)(set, &|| format!("L3:{name}")),
            // A call whose namespace lowering dropped (`console.log(..)` is
            // a call of `log` off the global scope): a `console` member is
            // core, a `Math` call that reaches here names no table (the
            // namespace's calls are intrinsic nodes; the detector reads it
            // the same way), any other namespace's member names it.
            None if ix.entries.contains_key(format!("console.{name}").as_str())
                || ix.entries.contains_key(format!("Math.{name}").as_str()) => {}
            None => self.global_value(name),
        }
    }

    /// Static member `key` of global `name`.
    fn global_member(&mut self, name: &str, key: &str, e: &Expr) {
        let ix = index();
        if ix.entry(name).is_none() && !ix.roots.contains_key(name) {
            // A global the realm does not declare (a program global).
            return;
        }
        if let Some(own) = families::entry_families(name).filter(|set| *set != 0) {
            // A holder of a family: the family, and what the member is and
            // bears; a member the realm does not declare is the whole holder.
            let member = ix.member(name, key).unwrap_or_else(|| ix.root(name));
            (self.mark)(own | member, &|| format!("L3:{name}.{key}"));
            return;
        }
        if key == "prototype" && self.member_receivers.contains(&addr(e)) {
            // `Name.prototype.member`: the parent names the member.
            return;
        }
        match ix.member(name, key) {
            Some(set) => (self.mark)(set, &|| format!("L3:{name}.{key}")),
            // Not a declared member: the holder as a whole.
            None => (self.mark)(ix.root(name), &|| {
                format!("L3:{name}.{key} is not a declared member of {name}")
            }),
        }
    }

    /// Pass 1 over one expression (parents come before children).
    fn expr(&mut self, e: &Expr) {
        let ix = index();
        // The node itself.
        match nodes::class(e) {
            Node::Core | Node::Computed => {}
            Node::All(why) => self.all(&|| format!("L3:{why}")),
            Node::Fam(ids) => {
                let set = ids.iter().fold(0, |s, id| s | families::family_bit(id));
                (self.mark)(set, &|| format!("L3:node {}", node_name(e)));
            }
            Node::Entry(name) => {
                let set = families::entry_families(name).unwrap_or(0) | ix.named(name).unwrap_or(0);
                (self.mark)(set, &|| format!("L3:node {} ({name})", node_name(e)));
            }
            Node::Spec(spec) => {
                let set = families::module_families(spec).unwrap_or(0);
                (self.mark)(set, &|| format!("L3:node {} (module {spec})", node_name(e)));
            }
            Node::Global => {
                if !self.receivers.contains(&addr(e)) {
                    self.all(&|| "L3:the global object is used as a value".into());
                }
            }
            Node::Module => match e {
                Expr::NativeModuleRef(spec) => {
                    let spec = if spec == "process.namespace" {
                        "process"
                    } else {
                        spec
                    };
                    self.module_spec(spec)
                }
                Expr::NativeMethodCall { module, .. } => self.module_spec(module),
                Expr::DynamicImport { paths, arg, .. } => {
                    if paths.is_empty() && const_str(arg).is_none() {
                        self.all(&|| "L3:import() of a specifier that is not a constant".into());
                    }
                    for spec in paths.iter().cloned().chain(const_str(arg)) {
                        // A program module's own text is scanned as a module.
                        if families::module_families(&spec).is_some() {
                            self.module_spec(&spec);
                        }
                    }
                }
                _ => {}
            },
        }
        // What it does to its operands.
        match e {
            Expr::ObjectKeys(x)
            | Expr::ObjectGetOwnPropertyNames(x)
            | Expr::ReflectOwnKeys(x)
            | Expr::ForInKeys(x)
                if self.global_object(x) =>
            {
                self.receivers.insert(addr(x));
            }
            Expr::Call { callee, .. }
            | Expr::CallSpread { callee, .. }
            | Expr::NewDynamic { callee, .. }
            | Expr::NewDynamicSpread { callee, .. }
                if self.named_global(callee).is_some() =>
            {
                self.used_in_place.insert(addr(callee));
            }
            Expr::TypeOf(x) if self.named_global(x).is_some() => {
                self.used_in_place.insert(addr(x));
            }
            Expr::InstanceOf {
                ty_expr: Some(x), ..
            } if self.named_global(x).is_some() => {
                self.used_in_place.insert(addr(x));
            }
            _ => {}
        }
        if let Some(object) = computed_receiver(e) {
            if self.sentinel(object) || self.global_object(object) {
                self.all(&|| "L3:the global object under a key that is not a constant".into());
            }
        }
        if let Some((object, key)) = static_member(e) {
            if self.sentinel(object) || self.global_object(object) {
                self.receivers.insert(addr(object));
            } else if let Some(name) = self.named_global(object) {
                self.member_receivers.insert(addr(object));
                self.global_member(&name, &key, e);
            } else if let Some((inner, proto)) = static_member(object) {
                // `Name.prototype.member`.
                if proto == "prototype" {
                    if let Some(name) = self.named_global(inner) {
                        if ix.entry(&format!("{name}.prototype.{key}")).is_some() {
                            self.member_receivers.insert(addr(object));
                            let entry = format!("{name}.prototype.{key}");
                            (self.mark)(ix.entry(&entry).unwrap_or(0), &|| format!("L3:{entry}"));
                        }
                    }
                }
            }
        }
        // The expression as a named global or as the global object.
        if self.global_object(e)
            && !matches!(e, Expr::GlobalThisExpr)
            && !self.receivers.contains(&addr(e))
        {
            self.all(&|| "L3:the global object is used as a value".into());
        }
        if let Some(name) = self.named_global(e) {
            if self.member_receivers.contains(&addr(e)) {
                // Its member was observed by the parent.
            } else if self.used_in_place.contains(&addr(e)) {
                self.global_in_place(&name);
            } else if matches!(e, Expr::PropertyGet { .. } | Expr::IndexGet { .. }) {
                self.global_value(&name);
            }
        }
        // A write or update of a global names it as a value.
        if let Expr::PropertySet {
            object, property, ..
        }
        | Expr::PropertyUpdate {
            object, property, ..
        } = e
        {
            if self.sentinel(object) || self.global_object(object) {
                self.global_value(property);
            }
        }
    }
}

/// The variant name of a node, for a reason.
fn node_name(e: &Expr) -> String {
    let text = format!("{e:?}");
    let end = text
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(text.len());
    text[..end].to_owned()
}

/// Every string literal of Debug text `text`.
fn strings(text: &str, out: &mut FxHashSet<String>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        let mut plain = true;
        while j < bytes.len() && bytes[j] != b'"' {
            if bytes[j] == b'\\' {
                plain = false;
                j += 1;
            }
            j += 1;
        }
        // A name or key has no escape.
        if plain && j <= bytes.len() && j - start <= 64 {
            if let Some(s) = text.get(start..j) {
                if !s.is_empty() && !out.contains(s) {
                    out.insert(s.to_owned());
                }
            }
        }
        i = j + 1;
    }
}

/// The name and static-key parts over the strings the text holds. Returns
/// the keys of prototype members whose holder is of a family: they count
/// once that family is live.
fn string_rules(found: &FxHashSet<String>, mark: &mut Mark<'_>) -> Vec<super::MemberKey> {
    let mut conditional = Vec::new();
    let ix = index();
    let mut sorted: Vec<&String> = found.iter().collect();
    sorted.sort_unstable();
    for s in sorted {
        // A holder of a family, by name (a constructed class, a type, an
        // `instanceof` operand): the family and what the holder declares
        // born. A member of another family has its key; the holder as a
        // value is a node (`Scan::global_value`).
        if families::entry_root(s) == s.as_str()
            && ix.roots.contains_key(s.as_str())
            && families::entry_families(s).is_some_and(|set| set != 0)
        {
            mark(
                families::entry_families(s).unwrap_or(0) | ix.named(s).unwrap_or(0),
                &|| format!("L3:the name {s}"),
            );
        }
        for &(holder, adds, entry) in ix.member_keys.get(s.as_str()).map_or(&[][..], |v| v) {
            if holder == 0 {
                mark(adds, &|| format!("L3:the key {s} ({entry})"));
            } else {
                conditional.push((holder, adds, entry));
            }
        }
    }
    conditional
}

/// Whether a body generation does not walk holds a member access under a
/// key that is a value, and the strings it holds.
fn unwalked(g: &Gen, found: &mut FxHashSet<String>) -> bool {
    let computed = std::cell::Cell::new(false);
    let mut visit = |e: &Expr| {
        if nodes::class(e) == Node::Computed {
            computed.set(true);
        }
    };
    for w in &g.unwalked {
        match w {
            PendingWalk::Fn(_, _, f, _, _) => {
                perry_hir::tdz_check::for_each_function_expr(f, &mut visit);
                strings(&format!("{:?}", f.body), found);
            }
            PendingWalk::Fields(_, c) => {
                perry_hir::tdz_check::for_each_class_expr(c, &mut visit);
                strings(&format!("{:?}{:?}", c.fields, c.static_fields), found);
            }
            PendingWalk::Closure(_, e) => {
                perry_hir::tdz_check::for_each_expr_in(e, &mut visit);
                strings(&format!("{e:?}"), found);
            }
            #[cfg(test)]
            PendingWalk::OwnedClosure(_, e) => {
                perry_hir::tdz_check::for_each_expr_in(e, &mut visit);
                strings(&format!("{e:?}"), found);
            }
            PendingWalk::Module(mi) => {
                let module = g.modules[*mi as usize].1;
                perry_hir::tdz_check::for_each_stmts_expr(&module.init, &mut visit);
                strings(&format!("{:?}", module.init), found);
            }
        }
    }
    computed.get()
}

/// L3 over the whole program. Returns the member keys that count once
/// their holder's family is live.
pub(super) fn scan(g: &Gen, mark: &mut Mark<'_>) -> Vec<super::MemberKey> {
    let ix = index();
    let interp_linked = g.m.interp_linked;
    let mut found = FxHashSet::default();
    for (_, module) in &g.modules {
        // Import and re-export specifiers of modules the runtime provides.
        for import in &module.imports {
            if import.type_only || import.runtime_erased {
                continue;
            }
            let spec = import.source.as_str();
            let known = families::module_families(spec).is_some();
            if import.is_native || (known && import.resolved_path.is_none()) {
                match families::module_families(spec) {
                    Some(set) => {
                        let root = format!("node:{}", spec.strip_prefix("node:").unwrap_or(spec));
                        mark(set | ix.root(&root), &|| format!("L3:import of {spec}"));
                    }
                    None => mark(families::all_families(), &|| {
                        format!("L3:import of {spec}, a module that is not classified")
                    }),
                }
            }
        }
        let mut scan = Scan {
            module,
            mark: &mut *mark,
            interp_linked,
            receivers: Default::default(),
            member_receivers: Default::default(),
            used_in_place: Default::default(),
        };
        perry_hir::for_each_module_expr(module, &mut |e| scan.expr(e));
        strings(
            &format!(
                "{:?}{:?}{:?}{:?}",
                module.init, module.functions, module.classes, module.globals
            ),
            &mut found,
        );
    }
    let conditional = string_rules(&found, mark);
    // Bodies generation does not walk: computed keys, and with the
    // interpreter linked the Function constructor off any function.
    let mut unwalked_strings = FxHashSet::default();
    if unwalked(g, &mut unwalked_strings) {
        for &(set, entry) in &ix.computed {
            mark(set, &|| {
                format!("L3:a computed key in a body that is not walked may name {entry}")
            });
        }
        if interp_linked {
            mark(families::all_families(), &|| {
                "L3:a computed key in a body that is not walked, with the interpreter linked".into()
            });
        }
    }
    if interp_linked && unwalked_strings.contains("constructor") {
        mark(families::all_families(), &|| {
            "L3:constructor is read in a body that is not walked, with the interpreter linked"
                .into()
        });
    }
    conditional
}
