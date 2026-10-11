//! Computed names are admission hints, never constant-folded property keys.
//! Unknown keys keep the complete runtime read/write path. Harvest literal
//! choices instead of admitting every string for a symbol or an opaque key.
use super::ClassAccessorNames;
use perry_hir::{ClassComputedMemberKind, Expr, Module, Stmt};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Facts {
    bindings: HashMap<u32, Expr>,
    returns: HashMap<u32, Vec<Expr>>,
    properties: Vec<(String, Expr)>,
    accessors: Vec<(Expr, bool, bool)>,
    class_fields: HashMap<String, Vec<String>>,
    visited_functions: HashSet<u32>,
}

impl Facts {
    fn bindings_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Let {
                    id, init: Some(e), ..
                } => {
                    let value = match e {
                        Expr::Closure { func_id, .. } => Expr::FuncRef(*func_id),
                        _ => e.clone(),
                    };
                    self.bindings.insert(*id, value);
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.bindings_stmts(then_branch);
                    if let Some(body) = else_branch {
                        self.bindings_stmts(body);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => self.bindings_stmts(body),
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        self.bindings_stmts(std::slice::from_ref(init.as_ref()));
                    }
                    self.bindings_stmts(body);
                }
                Stmt::Labeled { body, .. } => {
                    self.bindings_stmts(std::slice::from_ref(body.as_ref()))
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.bindings_stmts(body);
                    if let Some(c) = catch {
                        self.bindings_stmts(&c.body);
                    }
                    if let Some(body) = finally {
                        self.bindings_stmts(body);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        self.bindings_stmts(&c.body);
                    }
                }
                _ => {}
            }
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        self.bindings_stmts(stmts);
        for stmt in stmts {
            perry_hir::walker::stmt_any_expr(stmt, &mut |e| {
                self.expr(e);
                false
            });
        }
    }

    fn function(&mut self, id: u32, body: &[Stmt]) {
        if !self.visited_functions.insert(id) {
            return;
        }
        self.returns.insert(
            id,
            body.iter()
                .filter_map(|s| {
                    if let Stmt::Return(Some(e)) = s {
                        Some(e.clone())
                    } else {
                        None
                    }
                })
                .collect(),
        );
        self.stmts(body);
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Closure { func_id, body, .. } => self.function(*func_id, body),
            Expr::Object(props) => self.properties.extend(props.iter().cloned()),
            Expr::ObjectSpread { parts } => self.properties.extend(
                parts
                    .iter()
                    .filter_map(|(k, v)| k.as_ref().map(|k| (k.clone(), v.clone()))),
            ),
            Expr::PropertySet {
                property, value, ..
            } => self.properties.push((property.clone(), (**value).clone())),
            Expr::New {
                class_name, args, ..
            } => {
                if let Some(fields) = self.class_fields.get(class_name) {
                    self.properties
                        .extend(fields.iter().cloned().zip(args.iter().cloned()));
                }
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                    if name == "js_object_define_accessor" && args.len() == 4 {
                        // Named object accessors already keep their existing
                        // path. Extend admission for computed expressions;
                        // widening unrelated named object reads adds miss
                        // guards without helping the computed-key workload.
                        if !matches!(args[1], Expr::String(_)) {
                            self.accessors.push((
                                args[1].clone(),
                                !matches!(args[2], Expr::Undefined),
                                !matches!(args[3], Expr::Undefined),
                            ));
                        }
                    } else if name == "js_object_literal_define" && args.len() == 3 {
                        if let Expr::String(key) = &args[1] {
                            self.properties.push((key.clone(), args[2].clone()));
                        }
                    }
                }
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |e| self.expr(e));
    }

    fn names(
        &self,
        e: &Expr,
        properties: &HashMap<String, HashSet<String>>,
        depth: usize,
    ) -> HashSet<String> {
        if depth > 8 {
            return HashSet::new();
        }
        match e {
            Expr::String(name) => HashSet::from([name.clone()]),
            Expr::LocalGet(id) => self
                .bindings
                .get(id)
                .map(|e| self.names(e, properties, depth + 1))
                .unwrap_or_default(),
            Expr::PropertyGet { property, .. } => {
                properties.get(property).cloned().unwrap_or_default()
            }
            Expr::Call { callee, .. } => {
                let callee = if let Expr::LocalGet(id) = callee.as_ref() {
                    self.bindings.get(id).unwrap_or(callee)
                } else {
                    callee
                };
                if let Expr::FuncRef(id) = callee {
                    self.returns
                        .get(id)
                        .into_iter()
                        .flatten()
                        .flat_map(|e| self.names(e, properties, depth + 1))
                        .collect()
                } else {
                    HashSet::new()
                }
            }
            _ => HashSet::new(),
        }
    }
}

pub(super) fn collect<'m>(modules: impl IntoIterator<Item = &'m Module>) -> ClassAccessorNames {
    let modules: Vec<_> = modules.into_iter().collect();
    let mut names = ClassAccessorNames::default();
    let mut facts = Vec::new();
    for module in &modules {
        let mut f = Facts::default();
        for class in &module.classes {
            f.class_fields.insert(
                class.name.clone(),
                class.fields.iter().map(|f| f.name.clone()).collect(),
            );
            names
                .getters
                .extend(class.getters.iter().map(|(n, _)| n.clone()));
            names
                .setters
                .extend(class.setters.iter().map(|(n, _)| n.clone()));
            for member in &class.computed_members {
                let (get, set) = match member.kind {
                    ClassComputedMemberKind::Getter => (true, false),
                    ClassComputedMemberKind::Setter => (false, true),
                    ClassComputedMemberKind::Method => continue,
                };
                f.accessors.push((member.key_expr.clone(), get, set));
            }
        }
        f.stmts(&module.init);
        for function in &module.functions {
            f.function(function.id, &function.body);
        }
        for class in &module.classes {
            for function in class
                .constructor
                .iter()
                .chain(class.methods.iter())
                .chain(class.static_methods.iter())
                .chain(class.getters.iter().map(|(_, f)| f))
                .chain(class.setters.iter().map(|(_, f)| f))
                .chain(class.computed_members.iter().map(|m| &m.function))
            {
                f.function(function.id, &function.body);
            }
        }
        facts.push(f);
    }
    // These are candidate strings, not an assertion about a property's live
    // value. The emitted shape/pair/slot guards authenticate every answer;
    // keys with other values still use the complete runtime path.
    let mut properties: HashMap<String, HashSet<String>> = HashMap::new();
    for _ in 0..3 {
        let mut next = properties.clone();
        for f in &facts {
            for (key, value) in &f.properties {
                next.entry(key.clone())
                    .or_default()
                    .extend(f.names(value, &properties, 0));
            }
        }
        properties = next;
    }
    for f in &facts {
        for (key, get, set) in &f.accessors {
            let candidates = f.names(key, &properties, 0);
            if *get {
                names.getters.extend(candidates.iter().cloned());
            }
            if *set {
                names.setters.extend(candidates);
            }
        }
    }
    names
}
