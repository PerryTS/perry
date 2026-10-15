//! Object model step 5, case 2c (OBJECT_MODEL_2026-10-13.md §3.7): the
//! program-wide key colouring.
//!
//! A key read at sites whose receiver nothing proves gets one program-wide
//! slot (its colour) and a bit in every layout record's colour mask. A read
//! site of a coloured key (`expr/layout_read.rs`) tests the receiver layout's
//! bit and loads the colour slot: no call, no site state.
//!
//! The colour of a key is the position it has in most of the program's
//! layouts that hold it: class instances (the class chain's fields, ancestors
//! first), object literals (their key order) and constructor functions (their
//! `this.k = ...` stores in order). A layout sets a key's bit only when the
//! key really sits at its colour slot there, as data, inline: the runtime
//! decides that from the layout's own immutable key list
//! (`perry-runtime` `object/layout_probe.rs`), so a wrong guess here costs a
//! miss, never a wrong answer.
//!
//! The colours are static data: every module's atom records carry them
//! (`layout_desc::atom`), so the runtime knows the colour of each key without
//! a table of its own.

use std::collections::{BTreeMap, HashMap};

use perry_hir::{Class, Expr, Module as HirModule, Stmt};

/// Mask bits a layout record carries (two words); the top bit is reserved.
pub const MAX_COLOURED_KEYS: usize = 127;

/// The program's key colouring. One value for every module of a compile;
/// its digest is in every module's object-cache key.
#[derive(Debug, Default, Clone)]
pub struct ProgramLayouts {
    /// Kept for the object-cache key (no static class families here).
    pub families: BTreeMap<u32, u32>,
    /// Coloured key -> (mask bit, slot).
    pub colored: BTreeMap<String, (u32, u32)>,
    /// Mask words a record carries (0 when nothing is coloured).
    pub mask_words: u32,
    pub digest: u64,
    /// Candidate layouts and keys read (census).
    pub layout_count: u32,
    pub read_keys: u32,
}

impl ProgramLayouts {
    /// The (mask bit, slot) of a coloured key.
    pub fn colored(&self, key: &str) -> Option<(u32, u32)> {
        self.colored.get(key).copied()
    }

    /// One line for `PERRY_LAYOUT_REPORT`.
    pub fn report(&self) -> String {
        format!(
            "perry: key colouring: {} layouts, {} keys read, {} coloured",
            self.layout_count,
            self.read_keys,
            self.colored.len()
        )
    }
}

/// Is the colouring switched off (`PERRY_LAYOUT_READS=0`)?
fn disabled() -> bool {
    std::env::var("PERRY_LAYOUT_READS").as_deref() == Ok("0")
}

/// Visit every statement and expression of `body`, closures included.
fn visit_stmts<'a>(
    body: &'a [Stmt],
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    for s in body {
        visit_stmt(s, on_stmt, on_expr);
    }
}

fn visit_stmt<'a>(
    s: &'a Stmt,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    on_stmt(s);
    match s {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                visit_expr(e, on_stmt, on_expr);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => visit_expr(e, on_stmt, on_expr),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, on_stmt, on_expr);
            visit_stmts(then_branch, on_stmt, on_expr);
            if let Some(b) = else_branch {
                visit_stmts(b, on_stmt, on_expr);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            visit_expr(condition, on_stmt, on_expr);
            visit_stmts(body, on_stmt, on_expr);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt(i, on_stmt, on_expr);
            }
            if let Some(c) = condition {
                visit_expr(c, on_stmt, on_expr);
            }
            if let Some(u) = update {
                visit_expr(u, on_stmt, on_expr);
            }
            visit_stmts(body, on_stmt, on_expr);
        }
        Stmt::Labeled { body, .. } => visit_stmt(body, on_stmt, on_expr),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_stmts(body, on_stmt, on_expr);
            if let Some(c) = catch {
                visit_stmts(&c.body, on_stmt, on_expr);
            }
            if let Some(f) = finally {
                visit_stmts(f, on_stmt, on_expr);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, on_stmt, on_expr);
            for c in cases {
                if let Some(t) = &c.test {
                    visit_expr(t, on_stmt, on_expr);
                }
                visit_stmts(&c.body, on_stmt, on_expr);
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn visit_expr<'a>(
    e: &'a Expr,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    on_expr(e);
    if let Expr::Closure { body, .. } = e {
        visit_stmts(body, on_stmt, on_expr);
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, on_stmt, on_expr));
}

/// Every function body of a module: init, functions, class members.
fn module_bodies(hir: &HirModule) -> Vec<&[Stmt]> {
    let mut out: Vec<&[Stmt]> = vec![hir.init.as_slice()];
    for f in &hir.functions {
        out.push(&f.body);
    }
    for c in &hir.classes {
        for f in c
            .constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.static_methods.iter())
        {
            out.push(&f.body);
        }
        for (_, f) in c.getters.iter().chain(c.setters.iter()) {
            out.push(&f.body);
        }
    }
    out
}

/// The `this.k = ...` keys a function body stores, in order (closures not
/// entered: their `this` is another binding unless an arrow, and an arrow's
/// stores follow the body's own anyway).
fn this_store_keys(body: &[Stmt]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    fn walk(s: &Stmt, keys: &mut Vec<String>) {
        let mut note = |k: &str| {
            if !keys.iter().any(|x| x == k) {
                keys.push(k.to_string());
            }
        };
        match s {
            Stmt::Expr(Expr::PropertySet {
                object, property, ..
            }) if matches!(object.as_ref(), Expr::This) => note(property),
            Stmt::Expr(Expr::PutValueSet { target, key, .. })
                if matches!(target.as_ref(), Expr::This) =>
            {
                if let Expr::String(k) = key.as_ref() {
                    note(k)
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for t in then_branch {
                    walk(t, keys);
                }
                if let Some(b) = else_branch {
                    for t in b {
                        walk(t, keys);
                    }
                }
            }
            _ => {}
        }
    }
    for s in body {
        walk(s, &mut keys);
    }
    keys
}

/// A class instance's keys in birth order: the chain's fields, ancestors
/// first, then the constructor's own stores.
fn class_keys(c: &Class, by_id: &HashMap<u32, &Class>) -> Vec<String> {
    let mut chain: Vec<&Class> = vec![c];
    let mut cur = c;
    while let Some(p) = cur.extends.and_then(|id| by_id.get(&id).copied()) {
        if chain.len() > 32 || chain.iter().any(|x| std::ptr::eq(*x, p)) {
            break;
        }
        chain.push(p);
        cur = p;
    }
    let mut keys: Vec<String> = Vec::new();
    for k in chain.iter().rev() {
        for f in &k.fields {
            if f.is_private || f.key_expr.is_some() {
                continue;
            }
            keys.push(f.name.clone());
        }
        if let Some(ctor) = k.constructor.as_ref() {
            for s in this_store_keys(&ctor.body) {
                if !keys.contains(&s) {
                    keys.push(s);
                }
            }
        }
    }
    keys
}

/// The program's key colouring over every module's final HIR.
pub fn compute<'a>(modules: impl IntoIterator<Item = &'a HirModule>) -> ProgramLayouts {
    if disabled() {
        return ProgramLayouts::default();
    }
    let modules: Vec<&HirModule> = modules.into_iter().collect();
    let mut reads: HashMap<String, u64> = HashMap::new();
    let mut layouts: Vec<Vec<String>> = Vec::new();
    let mut by_id: HashMap<u32, &Class> = HashMap::new();
    for hir in &modules {
        for c in &hir.classes {
            by_id.insert(c.id, c);
        }
    }
    for hir in &modules {
        for body in module_bodies(hir) {
            visit_stmts(body, &mut |_| {}, &mut |e| match e {
                Expr::PropertyGet { property, .. } => {
                    *reads.entry(property.clone()).or_default() += 1;
                }
                Expr::Object(props) => {
                    let mut keys: Vec<String> = Vec::new();
                    for (k, _) in props {
                        if !keys.contains(k) {
                            keys.push(k.clone());
                        }
                    }
                    layouts.push(keys);
                }
                Expr::Closure { body, .. } => {
                    let keys = this_store_keys(body);
                    if !keys.is_empty() {
                        layouts.push(keys);
                    }
                }
                _ => {}
            });
        }
        for f in &hir.functions {
            let keys = this_store_keys(&f.body);
            if !keys.is_empty() {
                layouts.push(keys);
            }
        }
        for c in &hir.classes {
            let keys = class_keys(c, &by_id);
            if !keys.is_empty() {
                layouts.push(keys);
            }
        }
    }
    // Per key: how many layouts hold it at each position.
    let mut positions: HashMap<&str, BTreeMap<u32, u32>> = HashMap::new();
    for l in &layouts {
        for (pos, k) in l.iter().enumerate() {
            // A key a layout holds twice (a redeclared field) has no one
            // position there.
            if l.iter().filter(|x| *x == k).count() != 1 || !reads.contains_key(k) {
                continue;
            }
            if k.is_empty() || k.len() > 64 || k == "__proto__" || k == "length" || k == "size" {
                continue;
            }
            *positions
                .entry(k.as_str())
                .or_default()
                .entry(pos as u32)
                .or_default() += 1;
        }
    }
    let mut candidates: Vec<(&str, u64)> = positions
        .keys()
        .map(|k| (*k, reads.get(*k).copied().unwrap_or(0)))
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    candidates.truncate(MAX_COLOURED_KEYS);
    let mut out = ProgramLayouts {
        layout_count: layouts.len() as u32,
        read_keys: reads.len() as u32,
        ..Default::default()
    };
    let mut chosen: Vec<(&str, u32)> = candidates
        .iter()
        .map(|(k, _)| {
            let slot = positions[k]
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(p, _)| *p)
                .unwrap_or(0);
            (*k, slot)
        })
        .collect();
    chosen.sort();
    for (bit, (k, slot)) in chosen.iter().enumerate() {
        out.colored.insert((*k).to_string(), (bit as u32, *slot));
    }
    out.mask_words = if out.colored.is_empty() { 0 } else { 2 };
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    out.colored.hash(&mut h);
    out.digest = h.finish();
    if std::env::var_os("PERRY_LAYOUT_REPORT").is_some() {
        eprintln!("{}", out.report());
        if std::env::var("PERRY_LAYOUT_REPORT").as_deref() == Ok("2") {
            for (k, (bit, slot)) in &out.colored {
                eprintln!("perry: colour {k} bit {bit} slot {slot} reads {}", reads[k]);
            }
        }
    }
    out
}
