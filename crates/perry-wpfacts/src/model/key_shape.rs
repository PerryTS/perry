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

use super::*;
use perry_hir::{BinaryOp, Expr};

/// Glob pieces: `p0*p1*...*pn` (`""` where a substitution stands first or
/// last); one piece is an exact name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyShape {
    pub pieces: Vec<String>,
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
                    pieces: vec![s.clone()],
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
        lit.then_some(KeyShape { pieces })
    }

    /// Exact glob match: prefix, suffix, then each middle piece in order
    /// (the leftmost occurrence is complete for `*`-only globs).
    pub fn admits(&self, name: &str) -> bool {
        let p = &self.pieces;
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

    /// May a string of this shape be a Number key (`numeric_key`: Rust's
    /// f64 syntax, any case of `inf`/`infinity`/`nan`)?
    fn may_be_numeric(&self) -> bool {
        if self.pieces.len() == 1 {
            return crate::solve::numeric_key(&self.pieces[0]);
        }
        self.pieces.iter().all(|s| {
            s.chars()
                .all(|c| c.is_ascii_digit() || ".+-eEiInNfFtTyYaA".contains(c))
        })
    }
}

impl Model {
    /// The shape id of key expression `e` (0: none, any key).
    pub fn key_shape(&mut self, e: &Expr) -> KeyShapeId {
        self.key_shape_census[0] += 1;
        let Some(s) = KeyShape::of(e).filter(|s| !s.may_be_numeric()) else {
            return 0;
        };
        self.key_shape_census[1] += 1;
        if !s.admits("constructor") {
            self.key_shape_census[2] += 1;
        }
        if self.key_shapes.is_empty() {
            self.key_shapes.push(KeyShape { pieces: Vec::new() });
        }
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
    /// is a String that is no Number key: never an element or numeric key.
    /// The solver's other internal keys (`#...`, `\u{1}...`) stay possible.
    pub fn key_may_name_str(&self, key: Key, name: &str) -> bool {
        match key {
            Key::Named(a) => self.atoms[a as usize] == name,
            Key::Any(0) => true,
            Key::Any(s) => match name {
                ELEM | NUM_KEY => false,
                n if n.starts_with('#') || n.starts_with('\u{1}') => true,
                n => self.key_shapes[s as usize].admits(n),
            },
        }
    }

    pub fn key_may_name(&self, key: Key, a: Atom) -> bool {
        match key {
            Key::Named(b) => a == b,
            Key::Any(0) => true,
            Key::Any(_) => self.key_may_name_str(key, &self.atoms[a as usize]),
        }
    }
}
