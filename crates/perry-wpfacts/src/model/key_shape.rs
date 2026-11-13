//! The shape of a computed key built from literal parts (`o[type + "s"]`,
//! `` o[`${a}:${b}`] ``). `+` with a String operand is a String or a throw
//! (ECMA-262 13.15.3: ToPrimitive, then concatenation; ToString throws for a
//! Symbol), and a template is always a String; HIR desugars a template into
//! the same `+` chain (`lower_tpl`). Such a key is a string that starts with
//! its leading literal, ends with its trailing literal and holds every
//! literal part in order: a glob `p0*p1*...*pn`. It names only members whose
//! name matches (`Model::key_may_name`), the one question every lookup asks.
//!
//! A shape that may spell a Number key (`numeric_key`) is no shape: such a
//! key may be an index, and keeps the any-key path.
//!
//! A call of a module function is the String its body returns: when every
//! `return` of that body has a shape, the key is one of them, or
//! `"undefined"` (a bare `return` or the end of the body); a union of globs.

use super::*;
use perry_hir::{BinaryOp, Expr, Stmt};

/// A union of globs, each glob's pieces `p0*p1*...*pn` (`""` where a
/// substitution stands first or last); one piece is an exact name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyShape {
    pub globs: Vec<Vec<String>>,
}

enum Part {
    Lit(String),
    Hole,
}

/// The parts of a String-valued `+` chain; `false` when `e` may be no
/// String (a `+` without a String operand may add Numbers: it is one hole
/// of an enclosing concatenation).
fn str_parts(e: &Expr, out: &mut Vec<Part>) -> bool {
    match e {
        Expr::String(s) => {
            out.push(Part::Lit(s.clone()));
            true
        }
        Expr::TemplateStringCoerce(_) => {
            out.push(Part::Hole);
            true
        }
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => {
            let mut l = Vec::new();
            let mut r = Vec::new();
            let ls = str_parts(left, &mut l);
            let rs = str_parts(right, &mut r);
            if !ls && !rs {
                return false;
            }
            match ls {
                true => out.append(&mut l),
                false => out.push(Part::Hole),
            }
            match rs {
                true => out.append(&mut r),
                false => out.push(Part::Hole),
            }
            true
        }
        _ => false,
    }
}

impl KeyShape {
    /// The shape of key expression `e`: `None` unless it is a String built
    /// with at least one non-empty string-literal part, or a well-known
    /// symbol. `Symbol.iterator` & co. are HIR's `@@__perry_wk_<name>`
    /// sentinel (`read_key`), one symbol per name: no String, so the key
    /// names that sentinel only (never `__proto__`, never a string member).
    pub fn of(e: &Expr) -> Option<KeyShape> {
        if let Expr::SymbolFor(name) = e {
            return match name.as_ref() {
                Expr::String(s) if s.starts_with(crate::builtins::WK_PREFIX) => Some(KeyShape {
                    globs: vec![vec![s.clone()]],
                }),
                _ => None,
            };
        }
        let mut parts = Vec::new();
        if !str_parts(e, &mut parts) {
            return None;
        }
        let mut pieces = vec![String::new()];
        let mut lit = false;
        let mut hole_last = false;
        for p in parts {
            match p {
                Part::Lit(s) => {
                    lit |= !s.is_empty();
                    pieces.last_mut().expect("one piece").push_str(&s);
                    hole_last = false;
                }
                // Adjacent holes are one.
                Part::Hole if hole_last => {}
                Part::Hole => {
                    pieces.push(String::new());
                    hole_last = true;
                }
            }
        }
        lit.then_some(KeyShape {
            globs: vec![pieces],
        })
    }

    /// The String a call of `f` returns, when `f` is neither async nor a
    /// generator (its call returns a promise or an iterator, an object) and
    /// every `return` of its own body (`for_each_stmt_shallow`: nested
    /// functions return to their own callers) has a shape: one of those
    /// shapes, or `"undefined"` (a bare `return`, or the end of the body;
    /// `undefined` converts to that key). A throw returns no key.
    pub fn of_returns(f: &perry_hir::Function) -> Option<KeyShape> {
        if f.is_async || f.is_generator {
            return None;
        }
        let mut globs = vec![vec!["undefined".to_string()]];
        let mut ok = true;
        perry_hir::tdz_check::for_each_stmt_shallow(&f.body, &mut |s| {
            if let Stmt::Return(r) = s {
                match r.as_ref().map(KeyShape::of) {
                    None => {}
                    Some(Some(k)) => globs.extend(k.globs),
                    Some(None) => ok = false,
                }
            }
        });
        globs.sort();
        globs.dedup();
        ok.then_some(KeyShape { globs })
    }

    /// Does a glob of the union match `name`?
    pub fn admits(&self, name: &str) -> bool {
        self.globs.iter().any(|g| glob_admits(g, name))
    }

    /// May one string match both shapes? False only when every pair of
    /// globs is apart: an exact name the other does not admit, or first
    /// pieces (or last pieces) neither of which starts (ends) the other
    /// (rule M3: incompatible known prefixes are never equal, 7.2.13).
    pub fn meets(&self, o: &KeyShape) -> bool {
        self.globs.iter().any(|g| o.globs.iter().any(|h| glob_meets(g, h)))
    }

    /// May a string of this shape be a Number key (`numeric_key`)?
    pub(crate) fn may_be_numeric(&self) -> bool {
        self.globs.iter().any(|g| glob_may_be_numeric(g))
    }
}

fn glob_meets(g: &[String], h: &[String]) -> bool {
    match (g.len(), h.len()) {
        (1, _) => glob_admits(h, &g[0]),
        (_, 1) => glob_admits(g, &h[0]),
        _ => {
            let (gf, hf) = (&g[0], &h[0]);
            let (gl, hl) = (&g[g.len() - 1], &h[h.len() - 1]);
            (gf.starts_with(hf.as_str()) || hf.starts_with(gf.as_str()))
                && (gl.ends_with(hl.as_str()) || hl.ends_with(gl.as_str()))
        }
    }
}

/// Exact glob match: prefix, suffix, then each middle piece in order
/// (the leftmost occurrence is complete for `*`-only globs).
fn glob_admits(p: &[String], name: &str) -> bool {
    if p.len() == 1 {
        return name == p[0];
    }
    let (first, last) = (&p[0], &p[p.len() - 1]);
    if name.len() < first.len() + last.len()
        || !name.starts_with(first.as_str())
        || !name.ends_with(last.as_str())
    {
        return false;
    }
    let mut mid = &name[first.len()..name.len() - last.len()];
    for piece in &p[1..p.len() - 1] {
        match mid.find(piece.as_str()) {
            Some(i) => mid = &mid[i + piece.len()..],
            None => return false,
        }
    }
    true
}

/// May a string of glob `p` be a Number key (`numeric_key`: Rust's f64
/// syntax, any case of `inf`/`infinity`/`nan`)?
fn glob_may_be_numeric(p: &[String]) -> bool {
    if p.len() == 1 {
        return crate::solve::numeric_key(&p[0]);
    }
    p.iter().all(|s| {
        s.chars()
            .all(|c| c.is_ascii_digit() || ".+-eEiInNfFtTyYaA".contains(c))
    })
}

impl Model {
    /// The shape id of a computed key of shape `s` (0: none, any key).
    pub fn key_shape(&mut self, s: Option<KeyShape>) -> KeyShapeId {
        self.key_shape_census[0] += 1;
        let Some(s) = s.filter(|s| !s.may_be_numeric()) else {
            return 0;
        };
        self.key_shape_census[1] += 1;
        if !s.admits("constructor") {
            self.key_shape_census[2] += 1;
        }
        if self.key_shapes.is_empty() {
            self.key_shapes.push(KeyShape { globs: Vec::new() });
        }
        if let Some(&id) = self.key_shape_ids.get(&s) {
            return id;
        }
        let id = self.key_shapes.len() as KeyShapeId;
        self.key_shapes.push(s.clone());
        self.key_shape_ids.insert(s, id);
        id
    }

    /// The shape of a key that is any String (the glob `*`): it may spell
    /// every name, a Number key among them, but no symbol.
    pub fn any_string_shape(&mut self) -> KeyShapeId {
        if self.key_shapes.is_empty() {
            self.key_shapes.push(KeyShape { globs: Vec::new() });
        }
        let s = KeyShape {
            globs: vec![vec![String::new(), String::new()]],
        };
        if let Some(&id) = self.key_shape_ids.get(&s) {
            return id;
        }
        let id = self.key_shapes.len() as KeyShapeId;
        self.key_shapes.push(s.clone());
        self.key_shape_ids.insert(s, id);
        id
    }

    /// Rule K7: the shape of a key that is a String containing `c` (the
    /// glob `*c*`), interned as `any_string_shape` is: it may spell a
    /// Number key, and with `c` no IdentifierPart it spells no built-in
    /// member name.
    pub fn infix_shape(&mut self, c: char) -> KeyShapeId {
        if self.key_shapes.is_empty() {
            self.key_shapes.push(KeyShape { globs: Vec::new() });
        }
        let s = KeyShape {
            globs: vec![vec![String::new(), c.to_string(), String::new()]],
        };
        if let Some(&id) = self.key_shape_ids.get(&s) {
            return id;
        }
        let id = self.key_shapes.len() as KeyShapeId;
        self.key_shapes.push(s.clone());
        self.key_shape_ids.insert(s, id);
        id
    }

    /// May `key` name the member spelled `name`? The one question every
    /// lookup, builtin member and store asks of a computed key. A shaped key
    /// is a String, or a well-known symbol spelled exactly (`KeyShape::of`):
    /// a glob with a hole is a String and never names a symbol (HIR's
    /// `@@__perry_wk_<name>`). Only the any-String shape may be a Number
    /// key, an element or numeric key. The solver's other internal keys
    /// (`#...`, `\u{1}...`) stay possible.
    pub fn key_may_name_str(&self, key: Key, name: &str) -> bool {
        match key {
            Key::Named(a) => self.atoms[a as usize] == name,
            // An internal slot (`effects::internal_atom`) is no property:
            // no computed key names it (`Model::field`).
            Key::Any(_) if crate::builtins::is_internal_key(name) => false,
            Key::Any(0) => true,
            Key::Any(s) => match name {
                ELEM | NUM_KEY => self.key_shapes[s as usize].may_be_numeric(),
                n if n.starts_with('#') || n.starts_with('\u{1}') => true,
                n if n.starts_with(crate::builtins::WK_PREFIX) => self.key_shapes[s as usize]
                    .globs
                    .iter()
                    .any(|g| g.len() == 1 && g[0] == n),
                // The `[util.inspect.custom]` hook is a Symbol-keyed entry
                // or a class method HIR names `INSPECT_CUSTOM_METHOD` (the
                // class vtable slot perry-runtime formatting.rs reads); a
                // String key never installs it.
                crate::model::INSPECT_CUSTOM_METHOD => false,
                n => self.key_shapes[s as usize].admits(n),
            },
        }
    }

    /// DECISIONS 217 (owner 10-27 for `__proto__`, 11-05 for `prototype`):
    /// a computed key of unknown string never reads a prototype link, the
    /// `__proto__` accessor or a constructor's `prototype`. A computed read
    /// skips the member under that name; the named forms (`o.__proto__`,
    /// `C.prototype`, `C["prototype"]`, `Reflect.get(C, "prototype")`) read
    /// it.
    pub fn proto_link(name: &str) -> bool {
        matches!(name, "__proto__" | "prototype")
    }

    /// May a string of shape `a` equal one of shape `b`? (0: any key.)
    pub fn shapes_meet(&self, a: KeyShapeId, b: KeyShapeId) -> bool {
        a == b
            || a == 0
            || b == 0
            || self.key_shapes[a as usize].meets(&self.key_shapes[b as usize])
    }

    pub fn key_may_name(&self, key: Key, a: Atom) -> bool {
        match key {
            Key::Named(b) => a == b,
            Key::Any(_) => self.key_may_name_str(key, &self.atoms[a as usize]),
        }
    }
}
