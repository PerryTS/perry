//! Each abstract value keeps its resolved runtime shape with its kind.

use super::{Tok, TokKind};

pub struct TokenValue {
    pub kind: TokKind,
    pub shape: Option<usize>,
}

#[derive(Default)]
pub struct TokenValues(Vec<TokenValue>);

impl TokenValues {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, i: usize) -> Option<&TokKind> {
        self.0.get(i).map(|v| &v.kind)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TokKind> {
        self.0.iter().map(|v| &v.kind)
    }

    pub fn push(&mut self, kind: TokKind, shape: Option<usize>) {
        self.0.push(TokenValue { kind, shape });
    }

    pub fn shape(&self, t: Tok) -> Option<usize> {
        self.0[t as usize].shape
    }
}

impl std::ops::Index<usize> for TokenValues {
    type Output = TokKind;

    fn index(&self, i: usize) -> &TokKind {
        &self.0[i].kind
    }
}

/// One interning index, partitioned by the value kind. A key occurs in
/// exactly one bucket; runtime-function overlap never walks program values.
#[derive(Default)]
pub struct TokenIndex {
    ordinary: crate::pts::FxHashMap<TokKind, Tok>,
    builtin: crate::pts::FxHashMap<TokKind, Tok>,
}

impl TokenIndex {
    pub fn get(&self, kind: &TokKind) -> Option<&Tok> {
        match kind {
            TokKind::Builtin(_) | TokKind::BuiltinFn(_) => self.builtin.get(kind),
            _ => self.ordinary.get(kind),
        }
    }

    pub fn insert(&mut self, kind: TokKind, value: Tok) -> Option<Tok> {
        match kind {
            TokKind::Builtin(_) | TokKind::BuiltinFn(_) => self.builtin.insert(kind, value),
            _ => self.ordinary.insert(kind, value),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TokKind, &Tok)> {
        self.ordinary.iter().chain(self.builtin.iter())
    }

    pub fn builtin_values(&self) -> impl Iterator<Item = &Tok> {
        self.builtin.values()
    }
}

impl std::ops::Index<&TokKind> for TokenIndex {
    type Output = Tok;

    fn index(&self, kind: &TokKind) -> &Tok {
        self.get(kind).expect("interned token")
    }
}
