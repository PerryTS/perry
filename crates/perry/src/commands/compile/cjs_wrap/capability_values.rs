//! Own-property provenance for the original-source CommonJS refusal census.
//!
//! A descriptor exposes existing own values/accessors; it does not manufacture
//! a loader capability. Follow local calls, returns and literal object fields
//! before transforms erase them. Unknown receivers/escaped helper parameters
//! retain the original refusal. This proof never assumes CommonJS is closed.
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Function, Module, Param, Stmt};
use std::collections::{BTreeMap, BTreeSet};

type Value = BTreeSet<Origin>;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    Unknown,
    Undefined,
    Primitive,
    Global,
    Intrinsic(String),
    Namespace(String),
    Member,
    OwnAccessor,
    Function(u32),
    Object(usize),
}
fn one(v: Origin) -> Value {
    BTreeSet::from([v])
}
fn join(to: &mut Value, from: Value) -> bool {
    let n = to.len();
    to.extend(from);
    n != to.len()
}
#[derive(Clone, Copy)]
struct Body<'a> {
    params: &'a [Param],
    stmts: &'a [Stmt],
    strict: bool,
}
#[derive(Default)]
struct Shape {
    fields: BTreeMap<Option<String>, Value>,
    definite: BTreeSet<String>,
    escaped: bool,
}
struct Proof<'a> {
    module: &'a Module,
    functions: BTreeMap<u32, Body<'a>>,
    locals: BTreeMap<u32, Value>,
    returns: BTreeMap<u32, Value>,
    receivers: BTreeMap<u32, Value>,
    shapes: BTreeMap<usize, Shape>,
    descriptors: BTreeMap<usize, Value>,
    escaped_functions: BTreeSet<u32>,
    namespace_writes: BTreeMap<String, Value>,
    escaped_namespaces: BTreeSet<String>,
    record_locals: BTreeSet<u32>,
    exported_names: BTreeSet<String>,
    exported_locals: BTreeSet<u32>,
    changed: bool,
}
impl<'a> Proof<'a> {
    fn new(module: &'a Module) -> Self {
        let mut functions = BTreeMap::new();
        for f in &module.functions {
            functions.insert(f.id, Self::body(f));
        }
        fn expr<'a>(e: &'a Expr, functions: &mut BTreeMap<u32, Body<'a>>) {
            if let Expr::Closure {
                func_id,
                params,
                body,
                is_strict,
                ..
            } = e
            {
                functions.insert(
                    *func_id,
                    Body {
                        params,
                        stmts: body,
                        strict: *is_strict,
                    },
                );
                for s in body {
                    stmt(s, functions);
                }
            }
            walk_expr_children(e, &mut |c| expr(c, functions));
        }
        fn stmt<'a>(s: &'a Stmt, functions: &mut BTreeMap<u32, Body<'a>>) {
            perry_hir::walker::stmt_any_expr(s, &mut |e| {
                expr(e, functions);
                false
            });
        }
        for s in &module.init {
            stmt(s, &mut functions);
        }
        for f in &module.functions {
            for s in &f.body {
                stmt(s, &mut functions);
            }
        }
        Self {
            module,
            functions,
            locals: BTreeMap::new(),
            returns: BTreeMap::new(),
            receivers: BTreeMap::new(),
            shapes: BTreeMap::new(),
            descriptors: BTreeMap::new(),
            escaped_functions: BTreeSet::new(),
            namespace_writes: BTreeMap::new(),
            escaped_namespaces: BTreeSet::new(),
            record_locals: BTreeSet::new(),
            exported_locals: BTreeSet::new(),
            exported_names: module
                .exports
                .iter()
                .filter_map(|e| match e {
                    perry_hir::Export::Named { local, exported }
                        if !perry_hir::is_cjs_record_export(exported) =>
                    {
                        Some(local.clone())
                    }
                    _ => None,
                })
                .collect(),
            changed: false,
        }
    }
    fn body(f: &'a Function) -> Body<'a> {
        Body {
            params: &f.params,
            stmts: &f.body,
            strict: f.is_strict,
        }
    }
    fn local(&mut self, id: u32, value: Value) {
        self.changed |= join(self.locals.entry(id).or_default(), value);
    }
    fn field(&mut self, object: usize, key: Option<String>, value: Value) {
        self.changed |= join(
            self.shapes
                .entry(object)
                .or_default()
                .fields
                .entry(key)
                .or_default(),
            value,
        );
    }
    fn escape(&mut self, value: &Value) {
        for origin in value {
            match origin {
                Origin::Function(id) => {
                    self.changed |= self.escaped_functions.insert(*id);
                    if let Some(body) = self.functions.get(id).copied() {
                        for p in body.params {
                            self.local(p.id, one(Origin::Unknown));
                        }
                        self.changed |=
                            join(self.receivers.entry(*id).or_default(), one(Origin::Unknown));
                    }
                }
                Origin::Namespace(name) => {
                    self.changed |= self.escaped_namespaces.insert(name.clone());
                }
                Origin::Object(id) => {
                    let shape = self.shapes.entry(*id).or_default();
                    if !shape.escaped {
                        shape.escaped = true;
                        self.changed = true;
                    }
                }
                _ => {}
            }
        }
    }
    fn store(&mut self, object: &Value, key: Option<String>, value: Value) {
        for origin in object {
            match origin {
                // Null/undefined cannot receive a property; that path throws
                // before it can publish a value. Hoisted var bindings carry
                // this alternative alongside their initialized object.
                Origin::Undefined => {}
                Origin::Object(id) => self.field(*id, key.clone(), value.clone()),
                Origin::Namespace(name) => {
                    self.changed |= join(
                        self.namespace_writes.entry(name.clone()).or_default(),
                        value.clone(),
                    );
                }
                _ => self.escape(&value),
            }
        }
    }
    // Ordinary property reads include the prototype. Own descriptor reads
    // below deliberately do not; Object.prototype.constructor is not a
    // property of a fresh namespace copy.
    fn read(&self, object: &Value, key: Option<&str>, own: bool) -> Value {
        let mut out = Value::new();
        for origin in object {
            if let Origin::Namespace(name) = origin {
                if self.escaped_namespaces.contains(name) {
                    out.insert(Origin::Unknown);
                }
                out.extend(
                    self.namespace_writes
                        .get(name)
                        .into_iter()
                        .flatten()
                        .cloned(),
                );
            }
            out.extend(match origin {
                Origin::Undefined => Value::new(), // a normal read throws
                Origin::Namespace(module) if key == Some("default") => {
                    one(Origin::Namespace(module.clone()))
                }
                Origin::Namespace(_) if own => one(Origin::Member),
                Origin::Namespace(_)
                    if key.is_some_and(|k| !super::run_once_syntax::loader_property(k)) =>
                {
                    one(Origin::Member)
                }
                Origin::Object(id) => {
                    let Some(shape) = self.shapes.get(id) else {
                        continue;
                    };
                    let mut v = Value::new();
                    if shape.escaped {
                        v.insert(Origin::Unknown);
                    }
                    for (field, value) in &shape.fields {
                        if key.is_none() || field.is_none() || field.as_deref() == key {
                            v.extend(value.clone());
                        }
                    }
                    // A computed ordinary read can select a prototype key;
                    // a conditional store does not establish an own property.
                    if !own && !key.is_some_and(|key| shape.definite.contains(key)) {
                        v.insert(Origin::Unknown);
                    }
                    if v.is_empty() {
                        v.insert(if own {
                            Origin::Undefined
                        } else {
                            Origin::Unknown
                        });
                    }
                    v
                }
                Origin::Global if key == Some("Object") => one(Origin::Intrinsic("Object".into())),
                Origin::Intrinsic(path) => {
                    let p = format!("{path}.{}", key.unwrap_or("?"));
                    if matches!(
                        p.as_str(),
                        "Object.create"
                            | "Object.getOwnPropertyNames"
                            | "Object.prototype"
                            | "Object.prototype.hasOwnProperty"
                            | "Object.prototype.hasOwnProperty.call"
                    ) {
                        one(Origin::Intrinsic(p))
                    } else {
                        one(Origin::Unknown)
                    }
                }
                _ => one(Origin::Unknown),
            });
        }
        out
    }
    fn call(&mut self, callee: Value, args: &[Value], receiver: Value) -> Value {
        let mut out = Value::new();
        for origin in callee {
            match origin {
                Origin::Function(id) => {
                    if let Some(body) = self.functions.get(&id).copied() {
                        for (i, p) in body.params.iter().enumerate() {
                            self.local(
                                p.id,
                                if p.is_rest {
                                    one(Origin::Unknown)
                                } else {
                                    args.get(i)
                                        .cloned()
                                        .unwrap_or_else(|| one(Origin::Undefined))
                                },
                            );
                        }
                        let this = if body.strict {
                            receiver.clone()
                        } else {
                            one(Origin::Unknown)
                        };
                        self.changed |= join(self.receivers.entry(id).or_default(), this);
                        out.extend(self.returns.get(&id).cloned().unwrap_or_default());
                    } else {
                        out.insert(Origin::Unknown);
                    }
                }
                Origin::Intrinsic(ref path)
                    if matches!(
                        path.as_str(),
                        "Object.getOwnPropertyNames"
                            | "Object.prototype.hasOwnProperty"
                            | "Object.prototype.hasOwnProperty.call"
                    ) =>
                {
                    out.insert(Origin::Primitive);
                }
                Origin::OwnAccessor => {
                    out.insert(Origin::Member);
                }
                Origin::Undefined | Origin::Primitive => {} // not callable
                _ => {
                    for arg in args {
                        self.escape(arg);
                    }
                    out.insert(Origin::Unknown);
                }
            }
        }
        out
    }
    fn owned_record(&self, e: &Expr) -> bool {
        matches!(e, Expr::LocalGet(id) if self.record_locals.contains(id))
    }
    fn expr(&mut self, e: &'a Expr, function: Option<u32>) -> Value {
        let site = e as *const Expr as usize;
        match e {
            Expr::Undefined | Expr::Null => one(Origin::Undefined),
            Expr::Bool(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::Integer(_)
            | Expr::Number(_)
            | Expr::BigInt(_) => one(Origin::Primitive),
            Expr::LocalGet(id) => self.locals.get(id).cloned().unwrap_or_default(),
            Expr::LocalSet(id, value) => {
                let v = self.expr(value, function);
                self.local(*id, v.clone());
                if self.exported_locals.contains(id) {
                    self.escape(&v);
                }
                v
            }
            Expr::GlobalGet(_) => one(Origin::Global),
            Expr::This => function
                .and_then(|f| self.receivers.get(&f).cloned())
                .unwrap_or_else(|| one(Origin::Unknown)),
            Expr::NativeModuleRef(name)
                if perry_hir::is_node_builtin_module(name)
                    && !super::run_once_syntax::native_capability(name) =>
            {
                one(Origin::Namespace(name.clone()))
            }
            Expr::FuncRef(id) => one(Origin::Function(*id)),
            Expr::Closure {
                func_id,
                captures_this,
                ..
            } => {
                if *captures_this {
                    let v = function
                        .and_then(|f| self.receivers.get(&f).cloned())
                        .unwrap_or_else(|| one(Origin::Unknown));
                    self.changed |= join(self.receivers.entry(*func_id).or_default(), v);
                }
                one(Origin::Function(*func_id))
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.expr(condition, function);
                let mut v = self.expr(then_expr, function);
                v.extend(self.expr(else_expr, function));
                v
            }
            Expr::Logical { left, right, .. } => {
                let mut v = self.expr(left, function);
                v.extend(self.expr(right, function));
                v
            }
            Expr::Sequence(exprs) => {
                let mut v = one(Origin::Undefined);
                for e in exprs {
                    v = self.expr(e, function);
                }
                v
            }
            Expr::ScopedTemp { id, value, body } => {
                let v = self.expr(value, function);
                self.local(*id, v);
                self.expr(body, function)
            }
            Expr::PropertyGet {
                object, property, ..
            } => {
                let o = self.expr(object, function);
                // Function.call/apply/bind (including an opaque property key)
                // can invoke this function outside the direct-call graph.
                if !matches!(property.as_str(), "name" | "length") {
                    self.escape(
                        &o.iter()
                            .filter(|v| matches!(v, Origin::Function(_)))
                            .cloned()
                            .collect(),
                    );
                }
                self.read(&o, Some(property), false)
            }
            Expr::IndexGet { object, index } => {
                let o = self.expr(object, function);
                self.expr(index, function);
                self.escape(
                    &o.iter()
                        .filter(|v| matches!(v, Origin::Function(_)))
                        .cloned()
                        .collect(),
                );
                self.read(&o, key(index), false)
            }
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                let o = self.expr(object, function);
                let v = self.expr(value, function);
                if !(property == "require" && self.owned_record(object)) {
                    self.store(&o, Some(property.clone()), v.clone());
                }
                v
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                let o = self.expr(object, function);
                self.expr(index, function);
                let v = self.expr(value, function);
                self.store(&o, key(index).map(str::to_owned), v.clone());
                v
            }
            Expr::PutValueSet {
                target,
                key: k,
                value,
                receiver,
                ..
            } => {
                let o = self.expr(target, function);
                self.expr(k, function);
                self.expr(receiver, function);
                let v = self.expr(value, function);
                if !(key(k) == Some("require") && self.owned_record(target)) {
                    self.store(&o, key(k).map(str::to_owned), v.clone());
                }
                v
            }
            Expr::Call { callee, args, .. } => {
                let c = self.expr(callee, function);
                let a: Vec<_> = args.iter().map(|a| self.expr(a, function)).collect();
                let receiver = if let Expr::PropertyGet { object, .. }
                | Expr::IndexGet { object, .. } = callee.as_ref()
                {
                    self.expr(object, function)
                } else {
                    one(Origin::Undefined)
                };
                self.call(c, &a, receiver)
            }
            Expr::Object(fields) => {
                self.shapes.entry(site).or_default();
                for (k, e) in fields {
                    self.shapes
                        .entry(site)
                        .or_default()
                        .definite
                        .insert(k.clone());
                    let v = self.expr(e, function);
                    self.field(site, Some(k.clone()), v);
                }
                one(Origin::Object(site))
            }
            Expr::New {
                class_name, args, ..
            } if self
                .module
                .classes
                .iter()
                .any(|c| c.name == *class_name && c.is_literal_shape()) =>
            {
                self.shapes.entry(site).or_default();
                let class = self
                    .module
                    .classes
                    .iter()
                    .find(|c| c.name == *class_name)
                    .unwrap();
                for (field, arg) in class.fields.iter().zip(args) {
                    self.shapes
                        .entry(site)
                        .or_default()
                        .definite
                        .insert(field.name.clone());
                    let v = self.expr(arg, function);
                    self.field(site, Some(field.name.clone()), v);
                }
                one(Origin::Object(site))
            }
            Expr::Delete(operand) => {
                if let Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } =
                    operand.as_ref()
                {
                    let o = self.expr(object, function);
                    self.escape(&o);
                }
                self.expr(operand, function);
                one(Origin::Primitive)
            }
            Expr::ObjectGetOwnPropertyDescriptor(object, k) => {
                let o = self.expr(object, function);
                self.expr(k, function);
                self.changed |= join(self.descriptors.entry(site).or_default(), o.clone());
                let value = self.read(&o, key(k), true);
                self.field(site, Some("value".into()), value.clone());
                let accessors = if o.iter().all(|v| matches!(v, Origin::Namespace(_))) {
                    one(Origin::OwnAccessor)
                } else {
                    value
                };
                for field in ["get", "set"] {
                    self.field(site, Some(field.into()), accessors.clone());
                }
                one(Origin::Object(site))
            }
            Expr::ObjectDefineProperty(object, k, descriptor) => {
                let o = self.expr(object, function);
                self.expr(k, function);
                let d = self.expr(descriptor, function);
                let mut values = self.read(&d, Some("value"), true);
                let get = self.read(&d, Some("get"), true);
                // A later property store can invoke a setter with any value.
                let set = self.read(&d, Some("set"), true);
                self.escape(&set);
                values.extend(self.call(get, &[], o.clone()));
                self.store(&o, key(k).map(str::to_owned), values);
                o
            }
            Expr::ObjectAssign { target, sources } => {
                let o = self.expr(target, function);
                for source in sources {
                    let s = self.expr(source, function);
                    let values = self.read(&s, None, true);
                    self.store(&o, None, values);
                }
                o
            }
            // These operations inspect data but do not let a callback escape.
            Expr::Binary { .. }
            | Expr::Compare { .. }
            | Expr::Unary { .. }
            | Expr::TypeOf(_)
            | Expr::In { .. }
            | Expr::ForInKeys(_)
            | Expr::ObjectGetOwnPropertyNames(_)
            | Expr::ObjectGetOwnPropertySymbols(_) => {
                walk_expr_children(e, &mut |c| {
                    self.expr(c, function);
                });
                one(Origin::Primitive)
            }
            _ => {
                walk_expr_children(e, &mut |c| {
                    let v = self.expr(c, function);
                    self.escape(&v);
                });
                one(Origin::Unknown)
            }
        }
    }
    fn stmts(&mut self, stmts: &'a [Stmt], function: Option<u32>) {
        for s in stmts {
            self.stmt(s, function);
        }
    }
    fn stmt(&mut self, stmt: &'a Stmt, f: Option<u32>) {
        match stmt {
            Stmt::Let { id, name, init, .. } => {
                if self.exported_names.contains(name) {
                    self.exported_locals.insert(*id);
                }
                if let Some(e) = init {
                    // The original-source census refuses a module-record
                    // escape. Its generated `module = __cjs_module` alias
                    // installs require for open programs; this is not a user
                    // publication of that function into an unknown object.
                    if name == "module" && self.module.is_commonjs_wrap() {
                        if let Expr::LocalGet(record) = e {
                            self.record_locals.insert(*id);
                            self.record_locals.insert(*record);
                        }
                    }
                    let v = self.expr(e, f);
                    self.local(*id, v.clone());
                    if self.exported_names.contains(name) {
                        self.escape(&v);
                    }
                }
            }
            Stmt::Expr(e) => {
                self.expr(e, f);
            }
            Stmt::Return(e) => {
                let v = e
                    .as_ref()
                    .map_or_else(|| one(Origin::Undefined), |e| self.expr(e, f));
                if let Some(id) = f {
                    self.changed |= join(self.returns.entry(id).or_default(), v.clone());
                    if self.escaped_functions.contains(&id) {
                        self.escape(&v);
                    }
                }
            }
            Stmt::Throw(e) => {
                let v = self.expr(e, f);
                self.escape(&v);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                // The refusal census is over original source. The compiler's
                // guarded registry publication cannot be its own evidence
                // that a source function escaped to a dynamic loader.
                if matches!(condition, Expr::NativeMethodCall { module, object: None, method, .. }
                    if module == "__perry_runtime" && method == "cjsOpen")
                {
                    if let Some(b) = else_branch {
                        self.stmts(b, f);
                    }
                } else {
                    self.expr(condition, f);
                    self.stmts(then_branch, f);
                    if let Some(b) = else_branch {
                        self.stmts(b, f);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition, f);
                self.stmts(body, f);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i, f);
                }
                if let Some(e) = condition {
                    self.expr(e, f);
                }
                if let Some(e) = update {
                    self.expr(e, f);
                }
                self.stmts(body, f);
            }
            Stmt::Labeled { body, .. } => self.stmt(body, f),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body, f);
                if let Some(c) = catch {
                    if let Some((id, _)) = c.param {
                        self.local(id, one(Origin::Unknown));
                    }
                    self.stmts(&c.body, f);
                }
                if let Some(b) = finally {
                    self.stmts(b, f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, f);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(t, f);
                    }
                    self.stmts(&c.body, f);
                }
            }
            _ => {}
        }
    }
    fn safe_receiver(&self, value: &Value, seen: &mut BTreeSet<usize>) -> bool {
        !value.is_empty()
            && value.iter().all(|o| match o {
                Origin::Namespace(name) => {
                    !self.escaped_namespaces.contains(name)
                        && self.namespace_writes.get(name).is_none_or(|values| {
                            values.iter().all(|v| {
                                matches!(
                                    v,
                                    Origin::Undefined
                                        | Origin::Primitive
                                        | Origin::Member
                                        | Origin::Function(_)
                                )
                            })
                        })
                }
                Origin::Object(id) => {
                    if !seen.insert(*id) {
                        return true;
                    }
                    self.shapes.get(id).is_some_and(|s| {
                        !s.escaped
                            && s.fields.values().all(|v| {
                                !v.is_empty()
                                    && v.iter().all(|o| match o {
                                        Origin::Unknown | Origin::Global | Origin::Intrinsic(_) => {
                                            false
                                        }
                                        Origin::Object(_) => {
                                            self.safe_receiver(&one(o.clone()), seen)
                                        }
                                        _ => true,
                                    })
                            })
                    })
                }
                _ => false,
            })
    }
    fn solve(mut self) -> Option<usize> {
        for f in &self.module.functions {
            if f.is_exported || self.exported_names.contains(&f.name) {
                self.escape(&one(Origin::Function(f.id)));
            }
        }
        loop {
            self.changed = false;
            self.stmts(&self.module.init, None);
            let bodies: Vec<_> = self.functions.iter().map(|(id, b)| (*id, *b)).collect();
            for (id, body) in bodies {
                self.stmts(body.stmts, Some(id));
            }
            // Escape is transitive through a literal's own fields.
            let escaped: Vec<_> = self
                .shapes
                .values()
                .filter(|s| s.escaped)
                .flat_map(|s| s.fields.values().cloned())
                .collect();
            for value in escaped {
                self.escape(&value);
            }
            if !self.changed {
                break;
            }
        }
        self.descriptors
            .values()
            .all(|v| self.safe_receiver(v, &mut BTreeSet::new()))
            .then_some(self.descriptors.len())
    }
}
fn key(e: &Expr) -> Option<&str> {
    if let Expr::String(s) = e {
        Some(s)
    } else {
        None
    }
}

pub(crate) fn refine(module: &mut Module) {
    let Some(syntax) = &module.class_family_syntax else {
        return;
    };
    let candidates = syntax.descriptor_capability_uses;
    // Another refusal already keeps this module open. Avoid solving a
    // large dynamic program merely to reduce an observational count.
    if candidates == 0 || syntax.module_capability_uses != candidates {
        return;
    }
    if Proof::new(module).solve() == Some(candidates as usize) {
        module
            .class_family_syntax
            .as_mut()
            .unwrap()
            .module_capability_uses -= candidates;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lowered(source: &str) -> Module {
        let wrapped = super::super::wrap::wrap_commonjs_for_target(
            source,
            std::path::Path::new("/p/main.cjs"),
            None,
            false,
            None,
        );
        let ast = perry_parser::parse_typescript(&wrapped, "main.cjs").unwrap();
        let mut module = perry_hir::lower_module(&ast, "main", "/p/main.cjs").unwrap();
        module.class_family_syntax = Some(super::super::family_syntax::analyze(source));
        module
    }
    fn uses(source: &str) -> u32 {
        let mut module = lowered(source);
        refine(&mut module);
        module.class_family_syntax.unwrap().module_capability_uses
    }
    #[test]
    fn descriptors_of_builtin_namespaces_follow_call_arguments() {
        for source in [
            "const fs = require('fs'); console.log(Object.getOwnPropertyDescriptor(fs, 'readFileSync').value);",
            "function descriptor(object, key) { return Object.getOwnPropertyDescriptor(object, key); } console.log(descriptor(require('fs'), 'readFileSync').value);",
            "const fs = require('fs'); const ownKeys = Object.getOwnPropertyNames; ownKeys(fs); console.log(Object.getOwnPropertyDescriptor(fs, 'readFileSync').value);",
            "const fs = require('fs'); var copy = {}; Object.defineProperty(copy, 'default', {value: fs}); console.log(Object.getOwnPropertyDescriptor(fs, 'readFileSync').value);",
            "const out = {}; Object.defineProperty(out, 'read', Object.getOwnPropertyDescriptor(require('fs'), 'readFileSync')); console.log(Object.getOwnPropertyDescriptor(out, 'read').value);",
            "function outer(input) { const inner = function(value) { return Object.getOwnPropertyDescriptor(value, 'sep'); }; return inner(input); } console.log(outer(require('path')).value);",
        ] {
            assert!(lowered(source).class_family_syntax.unwrap().module_capability_uses > 0);
            assert_eq!(uses(source), 0, "{source}");
        }
    }
    #[test]
    fn unknown_receivers_and_escaped_helpers_keep_the_refusal() {
        for source in [
            "function descriptor(object, key) { return Object.getOwnPropertyDescriptor(object, key); } module.exports = descriptor;",
            "const fs = require('fs'); function descriptor(object) { return Object.getOwnPropertyDescriptor(object, 'readFileSync'); } descriptor(fs); descriptor(Object);",
            "const p = Object.getOwnPropertyDescriptor(Object, 'getPrototypeOf').value(function(){}); Object.getOwnPropertyDescriptor(p, 'constructor').value('return process')();",
            "const loader = require(process.argv[2]); console.log(loader);",
            "require.cache[process.argv[2]] = {};",
            "const get = Object.getOwnPropertyDescriptor; get(require('fs'), 'readFileSync');",
            "const fs = require('fs'); fs.default = Object; Object.getOwnPropertyDescriptor(fs.default, 'getPrototypeOf');",
            "const fs = require('fs'); unknown(fs); Object.getOwnPropertyDescriptor(fs, 'readFileSync');",
            "const holder = {f: x => require('fs')}; const key = process.argv[2]; const ns = holder[key](Object); Object.getOwnPropertyDescriptor(ns, 'getPrototypeOf');",
            "Object.prototype.f = Object; const holder = {}; if (process.argv[2]) holder.f = require('fs'); Object.getOwnPropertyDescriptor(holder.f, 'getPrototypeOf');",
            "Object.prototype.f = Object; const holder = {f: require('fs')}; delete holder.f; Object.getOwnPropertyDescriptor(holder.f, 'getPrototypeOf');",
            "function f(o) { return Object.getOwnPropertyDescriptor(o, 'constructor'); } f(require('fs')); f.call(null, Object);",
            "function f(o) { return Object.getOwnPropertyDescriptor(o, 'constructor'); } f(require('fs')); const x = {}; Object.defineProperty(x, 'p', {set: f}); x.p = Object;",
        ] { assert!(uses(source) > 0, "{source}"); }
    }
    #[test]
    fn exported_descriptor_helpers_keep_unknown_call_arguments() {
        for source in [
            "import fs from 'fs'; export function descriptor(o) { return Object.getOwnPropertyDescriptor(o, 'readFileSync'); } descriptor(fs);",
            "import fs from 'fs'; export let descriptor; descriptor = function(o) { return Object.getOwnPropertyDescriptor(o, 'readFileSync'); }; descriptor(fs);",
            "import fs from 'fs'; function descriptor(o) { return Object.getOwnPropertyDescriptor(o, 'readFileSync'); } descriptor(fs); export default { descriptor };",
        ] {
            let ast = perry_parser::parse_typescript(source, "main.mjs").unwrap();
            let mut module = perry_hir::lower_module(&ast, "main", "/p/main.mjs").unwrap();
            module.class_family_syntax = Some(super::super::family_syntax::analyze(source));
            refine(&mut module);
            assert!(module.class_family_syntax.unwrap().module_capability_uses > 0, "{source}");
        }
    }
    #[test]
    fn namespace_copy_helpers_do_not_open_the_program() {
        let source = include_str!("../../../../tests/fixtures/cjs_closed_support/02-wc.cjs");
        assert_eq!(
            lowered(source)
                .class_family_syntax
                .unwrap()
                .module_capability_uses,
            1
        );
        assert_eq!(uses(source), 0);
        let renamed = source
            .replace("__createBinding", "copyOwn")
            .replace("__importStar", "namespaceCopy")
            .replace("__setModuleDefault", "setDefault");
        assert_eq!(uses(&renamed), 0);
    }
    #[test]
    fn closed_wrappers_keep_commonjs_identity_without_the_loader_import() {
        let mut module = lowered("module.exports = require('fs');");
        super::super::super::cjs_link::resolve_open_sites(&mut module, true).unwrap();
        assert!(module.is_commonjs_wrap());
        assert!(!module.imports.iter().any(|i| i
            .source
            .strip_prefix("node:")
            .unwrap_or(&i.source)
            == "module"));
    }
}
