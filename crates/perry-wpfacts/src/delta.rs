//! Actual pending deliveries: ordered values and the subscribers present
//! when those values arrived. Later subscribers get the current value from
//! edge registration, then receive only subsequent deliveries.
use perry_hir::fx::FxHashSet;
use std::{rc::Rc, sync::Arc};
#[derive(Clone, Debug)]
pub(crate) enum Targets {
    Empty,
    Small(Vec<u32>),
    Shared(Rc<FxHashSet<u32>>),
}
impl Targets {
    pub(crate) fn capture(order: &[u32], set: Option<&Rc<FxHashSet<u32>>>) -> Self {
        if order.is_empty() {
            Self::Empty
        } else if order.len() <= 16 {
            Self::Small(order.to_vec())
        } else {
            Self::Shared(
                set.cloned()
                    .unwrap_or_else(|| Rc::new(order.iter().copied().collect())),
            )
        }
    }
    fn contains(&self, n: u32) -> bool {
        match self {
            Self::Empty => false,
            Self::Small(v) => v.contains(&n),
            Self::Shared(v) => v.contains(&n),
        }
    }
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Small(a), Self::Small(b)) => a == b,
            (Self::Shared(a), Self::Shared(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
    fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Sequence {
    chunks: Vec<Arc<Vec<u32>>>,
    len: usize,
}
#[derive(Clone, Debug)]
enum Values {
    Owned(Vec<u32>),
    Shared(Arc<Sequence>),
}
impl Values {
    fn len(&self) -> usize {
        match self {
            Self::Owned(v) => v.len(),
            Self::Shared(v) => v.len,
        }
    }
    fn prepare(&mut self) {
        if let Self::Owned(v) = self {
            *self = Self::Shared(Batch::new(std::mem::take(v)).0);
        }
    }
    fn push(&mut self, t: u32) {
        match self {
            Self::Owned(v) => v.push(t),
            Self::Shared(v) => {
                let v = Arc::make_mut(v);
                v.len += 1;
                if let Some(last) = v.chunks.last_mut() {
                    if let Some(last) = Arc::get_mut(last) {
                        last.push(t);
                        return;
                    }
                }
                v.chunks.push(Arc::new(vec![t]));
            }
        }
    }
    fn append(&mut self, batch: &Batch) -> u64 {
        let extra = batch.0.chunks.len().saturating_sub(1) as u64;
        match self {
            Self::Owned(v) if v.is_empty() => {
                *self = Self::Shared(batch.0.clone());
                extra
            }
            Self::Owned(v) => {
                let len = v.len() + batch.len();
                let mut chunks = Vec::with_capacity(1 + batch.0.chunks.len());
                chunks.push(Arc::new(std::mem::take(v)));
                chunks.extend(batch.0.chunks.iter().cloned());
                *self = Self::Shared(Arc::new(Sequence { chunks, len }));
                extra
            }
            Self::Shared(v) => {
                let copied = if Arc::strong_count(v) > 1 {
                    v.chunks.len().saturating_sub(1) as u64
                } else {
                    0
                };
                let v = Arc::make_mut(v);
                v.chunks.extend(batch.0.chunks.iter().cloned());
                v.len += batch.len();
                copied + extra
            }
        }
    }
}
#[derive(Clone, Debug)]
struct Event {
    values: Values,
    targets: Targets,
}
#[derive(Clone, Debug, Default)]
pub struct Delta {
    events: Vec<Event>,
    len: usize,
}
pub(crate) struct Batch(Arc<Sequence>);
impl Batch {
    pub(crate) fn new(tokens: Vec<u32>) -> Self {
        let len = tokens.len();
        Self(Arc::new(Sequence {
            chunks: vec![Arc::new(tokens)],
            len,
        }))
    }
    pub(crate) fn len(&self) -> usize {
        self.0.len
    }
    #[cfg(test)]
    pub(crate) fn tokens(&self) -> Tokens<'_> {
        Tokens::Chunks(&self.0)
    }
}
impl Delta {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn capacity(&self) -> usize {
        self.events.capacity() * std::mem::size_of::<Event>() / std::mem::size_of::<u32>()
    }
    pub fn clear(&mut self) {
        self.events.clear();
        self.len = 0;
    }
    pub fn push(&mut self, t: u32) {
        self.push_to(t, &Targets::Empty);
    }
    pub(crate) fn push_to(&mut self, t: u32, targets: &Targets) {
        self.len += 1;
        if let Some(last) = self.events.last_mut() {
            if last.targets.same(targets) {
                last.values.push(t);
                return;
            }
        }
        self.events.push(Event {
            values: Values::Owned(vec![t]),
            targets: targets.clone(),
        });
    }
    #[cfg(test)]
    pub(crate) fn append_batch(&mut self, batch: &Batch) -> u64 {
        self.append_to(batch, &Targets::Empty)
    }
    pub(crate) fn append_to(&mut self, batch: &Batch, targets: &Targets) -> u64 {
        if batch.len() == 0 {
            return 0;
        }
        self.len += batch.len();
        if let Some(last) = self.events.last_mut() {
            if last.targets.same(targets) {
                return last.values.append(batch);
            }
        }
        self.events.push(Event {
            values: Values::Shared(batch.0.clone()),
            targets: targets.clone(),
        });
        batch.0.chunks.len().saturating_sub(1) as u64
    }
    pub(crate) fn prepare(&mut self) {
        for e in &mut self.events {
            e.values.prepare();
        }
    }
    pub(crate) fn delivery_len(&self, dst: u32) -> usize {
        self.events
            .iter()
            .filter(|e| e.targets.contains(dst))
            .map(|e| e.values.len())
            .sum()
    }
    pub(crate) fn delivery_overhead(&self) -> u64 {
        self.events
            .iter()
            .filter(|e| !e.targets.is_empty())
            .count()
            .saturating_sub(1) as u64
    }
    pub(crate) fn batch_for(&self, dst: u32) -> Batch {
        let mut chunks = Vec::new();
        let mut len = 0;
        for e in &self.events {
            if e.targets.contains(dst) {
                let Values::Shared(v) = &e.values else {
                    unreachable!("prepare deliveries before copying")
                };
                len += v.len;
                chunks.extend(v.chunks.iter().cloned());
            }
        }
        Batch(Arc::new(Sequence { chunks, len }))
    }
    pub fn iter(&self) -> Iter<'_> {
        Tokens::Pending(self).iter()
    }
}
pub struct Iter<'a> {
    current: std::slice::Iter<'a, u32>,
    chunks: std::slice::Iter<'a, Arc<Vec<u32>>>,
    events: std::slice::Iter<'a, Event>,
    target: Option<u32>,
}
impl<'a> Iterator for Iter<'a> {
    type Item = &'a u32;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(t) = self.current.next() {
                return Some(t);
            }
            if let Some(c) = self.chunks.next() {
                self.current = c.iter();
                continue;
            }
            let e = self.events.next()?;
            if self.target.is_some_and(|n| !e.targets.contains(n)) {
                continue;
            }
            match &e.values {
                Values::Owned(v) => self.current = v.iter(),
                Values::Shared(v) => self.chunks = v.chunks.iter(),
            }
        }
    }
}
impl<'a> IntoIterator for &'a Delta {
    type Item = &'a u32;
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Tokens<'a> {
    Slice(&'a [u32]),
    Pending(&'a Delta),
    Delivery(&'a Delta, u32),
    #[cfg(test)]
    Chunks(&'a Sequence),
}
impl<'a> Tokens<'a> {
    pub(crate) fn iter(self) -> Iter<'a> {
        match self {
            Self::Slice(v) => Iter {
                current: v.iter(),
                chunks: [].iter(),
                events: [].iter(),
                target: None,
            },
            Self::Pending(v) => Iter {
                current: [].iter(),
                chunks: [].iter(),
                events: v.events.iter(),
                target: None,
            },
            Self::Delivery(v, n) => Iter {
                current: [].iter(),
                chunks: [].iter(),
                events: v.events.iter(),
                target: Some(n),
            },
            #[cfg(test)]
            Self::Chunks(v) => Iter {
                current: [].iter(),
                chunks: v.chunks.iter(),
                events: [].iter(),
                target: None,
            },
        }
    }
    pub(crate) fn len(self) -> usize {
        match self {
            Self::Slice(v) => v.len(),
            Self::Pending(v) => v.len(),
            Self::Delivery(v, n) => v.delivery_len(n),
            #[cfg(test)]
            Self::Chunks(v) => v.len,
        }
    }
    pub(crate) fn is_empty(self) -> bool {
        self.len() == 0
    }
    pub(crate) fn contains(self, t: &u32) -> bool {
        self.iter().any(|v| v == t)
    }
}
impl<'a> IntoIterator for Tokens<'a> {
    type Item = &'a u32;
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
