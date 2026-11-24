//! K3 (B), receiver family (owner premise 11-23): a method or accessor
//! that code outside the program's view gets hold of runs only with
//! `this` = an object of its class family, or nullish. Its family is the
//! objects whose chain holds it under its home key: its home holders (the
//! class prototype or class object, the literal, the object an accessor
//! literal is defined on), the objects it is stored onto under that key
//! (copy targets), every object whose prototypes hold a member, and the
//! Proxies whose target holds a member. A modelled call or store that
//! puts it outside that shape (a receiver outside the family, a store
//! under another key) leaves it ⊤ `this` as before: one way, monotone.
//! The runtime checks the family where it invokes a handed method or
//! accessor (the next K3 step); this solve assumes that check.
//!
//! The holders are recorded while solving, as tokens arrive at field and
//! prototype nodes (`Node::fam_watch`): at the fixpoint a field that holds
//! ⊤ has dropped the escaped method beside it (`escaped_beside_top`), so
//! the family cannot be read from the field contents then.

use crate::model::{prim, Atom, FnIdx, Model, NodeId, Tok, TokKind, TOP};
use crate::pts::{FxHashMap, FxHashSet};

/// `Node::fam_watch`: a field node (only function tokens matter).
pub(crate) const WATCH_FIELD: u8 = 1;
/// `Node::fam_watch`: a prototypes node (every token is a chain edge).
pub(crate) const WATCH_PROTO: u8 = 2;
/// The key of an any-key or number-keys field node: no home key.
pub(crate) const NO_HOME_KEY: Atom = u32::MAX - 1;

#[derive(Default)]
pub struct Family {
    /// The home keys (a class member's spelling and its well-known alias).
    keys: Vec<Atom>,
    /// Set once a modelled use put the function outside its family.
    off: bool,
    /// Its `this` nodes, once an escape gave it its family (`escape_this`).
    dsts: Vec<NodeId>,
    members: FxHashSet<Tok>,
    /// Tokens `escape_this` adds itself beside the family (published
    /// receivers, a receiver marker).
    extra: FxHashSet<Tok>,
    /// A member escaped: ⊤ may be that member, so an object whose
    /// prototypes hold ⊤ is a member too.
    via_top: bool,
    /// The `this` sizes the last check saw (`settle_family`).
    seen: Vec<(u32, u16)>,
    /// The `this` nodes of its other runs (a summarized body's instances,
    /// made for modelled calls, also later ones): checked, never given the
    /// family.
    watch: Vec<NodeId>,
    wseen: Vec<(u32, u16)>,
}

#[derive(Default)]
pub struct RecvFam {
    fams: FxHashMap<FnIdx, Family>,
    /// Watched node -> the object it is the field `key` (or the
    /// prototypes) of.
    owner: FxHashMap<NodeId, (Tok, Atom)>,
    /// Prototype p -> the objects whose prototypes hold p.
    rev_proto: FxHashMap<Tok, Vec<Tok>>,
    /// Member -> the functions whose family holds it.
    fam_of: FxHashMap<Tok, Vec<FnIdx>>,
    /// Objects whose prototypes hold ⊤.
    top_proto: Vec<Tok>,
    /// `settle_family` has looked at the summary instances and tokens
    /// before these.
    inst_cursor: usize,
    tok_cursor: usize,
    /// `PERRY_K3_FAMOFF`: print why a function left its family.
    pub(crate) explain_off: bool,
    /// `PERRY_K3_NOPROTO` (measurement only): `this` leaves out the class
    /// prototype object.
    pub(crate) no_proto: bool,
}

impl Model {
    /// Gen: function token `ft` is a method or accessor whose home key is
    /// `keys`; `holder`, when known here, holds it.
    pub(crate) fn rfam_home(&mut self, ft: Tok, keys: &[Atom], holder: Option<Tok>) {
        let TokKind::Func(f, _) = self.toks[ft as usize] else {
            return;
        };
        if self.funcs[f as usize].lexical_this || keys.is_empty() {
            return;
        }
        let e = self.rfam.fams.entry(f).or_default();
        for &k in keys {
            if !e.keys.contains(&k) {
                e.keys.push(k);
            }
        }
        if let Some(h) = holder {
            self.rfam_holding(ft, h, keys[0]);
        }
    }

    /// `n` is the field `a` (or, `WATCH_PROTO`, the prototypes) of `t`.
    pub(crate) fn rfam_watch(&mut self, n: NodeId, t: Tok, a: Atom, kind: u8) {
        self.nodes[n as usize].fam_watch = kind;
        self.rfam.owner.insert(n, (t, a));
    }

    /// `n` is an accessor slot of a property descriptor record (6.2.6.4
    /// FromPropertyDescriptor `get` / `set`): it holds the property's
    /// accessor as that, no member of the record; the key it gets is the
    /// one a define installs it under (`Cons::Accessor`).
    pub(crate) fn rfam_unwatch(&mut self, n: NodeId) {
        self.nodes[n as usize].fam_watch = 0;
        self.rfam.owner.remove(&n);
    }

    /// The tokens of `ts` arriving at node `n` the family reads.
    pub(crate) fn rfam_filter(&self, n: NodeId, ts: &mut dyn Iterator<Item = Tok>) -> Vec<Tok> {
        match self.nodes[n as usize].fam_watch {
            WATCH_PROTO => ts.collect(),
            WATCH_FIELD => ts
                .filter(|&t| {
                    matches!(self.toks[t as usize], TokKind::Func(f, _) if self.rfam.fams.contains_key(&f))
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Tokens arrived at watched node `n` (those dropped beside ⊤ too).
    pub(crate) fn rfam_arrived(&mut self, n: NodeId, ts: &[Tok]) {
        let Some(&(h, a)) = self.rfam.owner.get(&n) else {
            return;
        };
        if self.nodes[n as usize].fam_watch == WATCH_PROTO {
            for &p in ts {
                if p == TOP {
                    self.rfam.top_proto.push(h);
                    let fs: Vec<FnIdx> = self
                        .rfam
                        .fams
                        .iter()
                        .filter(|(_, r)| r.via_top && !r.off)
                        .map(|(&f, _)| f)
                        .collect();
                    for f in fs {
                        self.rfam_member(f, h);
                    }
                    continue;
                }
                self.rfam.rev_proto.entry(p).or_default().push(h);
                if let Some(fs) = self.rfam.fam_of.get(&p).cloned() {
                    for f in fs {
                        self.rfam_member(f, h);
                    }
                }
            }
            return;
        }
        for &ft in ts {
            self.rfam_holding(ft, h, a);
        }
    }

    /// `h` holds function token `ft` under key `a` (a field or accessor).
    pub(crate) fn rfam_holding(&mut self, ft: Tok, h: Tok, a: Atom) {
        let TokKind::Func(f, _) = self.toks[ft as usize] else {
            return;
        };
        let Some(r) = self.rfam.fams.get(&f) else {
            return;
        };
        if r.off {
            return;
        }
        if r.keys.contains(&a) {
            self.rfam_member(f, h);
        } else {
            self.rfam_off(f, || format!("held by {h} under another key {a}"));
        }
    }

    /// `t` and the objects whose chain reaches it join `f`'s family.
    fn rfam_member(&mut self, f: FnIdx, t: Tok) {
        let mut work = vec![t];
        while let Some(t) = work.pop() {
            let Some(r) = self.rfam.fams.get_mut(&f) else {
                return;
            };
            if r.off || !r.members.insert(t) {
                continue;
            }
            let dsts = r.dsts.clone();
            self.rfam.fam_of.entry(t).or_default().push(f);
            if self.fam_this(t) {
                for d in dsts {
                    self.add_tok(d, t);
                }
            }
            if let Some(cs) = self.rfam.rev_proto.get(&t) {
                work.extend(cs.iter().copied());
            }
        }
    }

    /// A modelled use outside the family: `this` is ⊤ as before.
    pub(crate) fn rfam_off(&mut self, f: FnIdx, why: impl FnOnce() -> String) {
        let Some(r) = self.rfam.fams.get_mut(&f) else {
            return;
        };
        if r.off {
            return;
        }
        r.off = true;
        r.members.clear();
        let dsts = std::mem::take(&mut r.dsts);
        if self.rfam.explain_off {
            eprintln!(
                "K3FAMOFF\t{}\t{}\t{}",
                self.funcs[f as usize].name,
                dsts.len(),
                why()
            );
        }
        for d in dsts {
            self.add_top(d, |_| {
                "this of a method used outside its class family".into()
            });
            self.add_prim(d, prim::ALL);
        }
    }

    /// `escape_this`: `f`'s `this` nodes `dsts` get its family and nullish
    /// instead of ⊤. False when `f` has no family (a plain function) or
    /// left it: the caller adds ⊤.
    pub(crate) fn rfam_escape_this(&mut self, f: FnIdx, dsts: &[NodeId]) -> bool {
        let Some(r) = self.rfam.fams.get_mut(&f) else {
            return false;
        };
        if r.off {
            return false;
        }
        for &d in dsts {
            if !r.dsts.contains(&d) {
                r.dsts.push(d);
                r.seen.push((u32::MAX, 0));
            }
        }
        let members: Vec<Tok> = r.members.iter().copied().collect();
        let members: Vec<Tok> = members.into_iter().filter(|&t| self.fam_this(t)).collect();
        for &d in dsts {
            for &t in &members {
                self.add_tok(d, t);
            }
            self.add_prim(d, prim::NULL | prim::UNDEF);
        }
        true
    }

    /// A member `this` may be (owner 11-23: "`this` = an object of its own
    /// class family"): every object whose chain holds the function under
    /// its home key, the class prototype object itself included.
    /// `PERRY_K3_NOPROTO` (measurement only, never a default) leaves the
    /// bare prototype out, for the owner to decide with numbers.
    fn fam_this(&self, t: Tok) -> bool {
        !(self.rfam.no_proto && matches!(self.toks[t as usize], TokKind::Proto(_)))
    }

    /// A token `escape_this` adds itself beside the family.
    pub(crate) fn rfam_extra(&mut self, f: FnIdx, t: Tok) {
        if let Some(r) = self.rfam.fams.get_mut(&f) {
            r.extra.insert(t);
        }
    }

    /// Once the worklist is empty: a family with an escaped member takes
    /// the objects whose prototypes hold ⊤; a Proxy whose target holds a
    /// member joins; a `this` that holds a value outside the family (a
    /// modelled call) leaves the family. Monotone.
    pub(crate) fn settle_family(&mut self) -> bool {
        let mut changed = false;
        let live: Vec<FnIdx> = self
            .rfam
            .fams
            .iter()
            .filter(|(_, r)| !r.off && !r.dsts.is_empty())
            .map(|(&f, _)| f)
            .collect();
        if live.is_empty() {
            return false;
        }
        // Every run of a family function is checked, the ones made later
        // too: a modelled call's receiver lands in its instance's `this`.
        for i in self.rfam.inst_cursor..self.sum.insts.len() {
            let (f, d) = (self.sum.insts[i].f, self.sum.insts[i].this);
            if let Some(r) = self.rfam.fams.get_mut(&f) {
                if !r.dsts.contains(&d) && !r.watch.contains(&d) {
                    r.watch.push(d);
                    r.wseen.push((u32::MAX, 0));
                }
            }
        }
        self.rfam.inst_cursor = self.sum.insts.len();
        // A class instance, a `new f()` instance (its chain is `f.prototype`)
        // or a subclass prototype object has its static chain
        // from birth; its prototypes node is made on first use only, so
        // make it here: the chain joins it to a member prototype's family
        // (`rfam_arrived`, `rev_proto`).
        let n_toks = self.toks.len();
        for t in self.rfam.tok_cursor..n_toks {
            let static_chain = match self.toks[t] {
                TokKind::Obj(tm, _) => {
                    matches!(
                        self.tmpls[tm as usize].kind,
                        crate::model::TmplKind::Class { .. }
                            | crate::model::TmplKind::FnInst { .. }
                    )
                }
                TokKind::Proto(tm) => self.tmpls[tm as usize].parent.is_some(),
                _ => false,
            };
            if static_chain && !self.proto_node.contains_key(&(t as Tok)) {
                self.proto_node(t as Tok);
                changed = true;
            }
        }
        self.rfam.tok_cursor = n_toks;
        // Proxies of members.
        for i in 0..self.proxies.len() {
            let Some(&p) = self.tok_index.get(&TokKind::Proxy(i as u32)) else {
                continue;
            };
            let tn = self.proxies[i].target;
            let mut fs: Vec<FnIdx> = Vec::new();
            for t in self.nodes[tn as usize].pts.to_sorted() {
                if let Some(v) = self.rfam.fam_of.get(&t) {
                    fs.extend(v.iter().copied());
                }
            }
            for f in fs {
                if self
                    .rfam
                    .fams
                    .get(&f)
                    .is_some_and(|r| !r.off && !r.members.contains(&p))
                {
                    self.rfam_member(f, p);
                    changed = true;
                }
            }
        }
        for f in live {
            let r = &self.rfam.fams[&f];
            if r.off {
                continue;
            }
            if !r.via_top && r.members.iter().any(|t| self.escaped.contains_key(t)) {
                self.rfam.fams.get_mut(&f).unwrap().via_top = true;
                for t in self.rfam.top_proto.clone() {
                    self.rfam_member(f, t);
                }
                changed = true;
            }
            let r = &self.rfam.fams[&f];
            if r.off {
                continue;
            }
            let mut seen = r.seen.clone();
            seen.extend_from_slice(&r.wseen);
            let mut bad: Option<(NodeId, Option<Tok>)> = None;
            for (i, &d) in r.dsts.iter().chain(&r.watch).enumerate() {
                let node = &self.nodes[d as usize];
                let now = (node.pts.len() as u32, node.prim);
                if seen[i] == now {
                    continue;
                }
                seen[i] = now;
                if node.prim & !(prim::NULL | prim::UNDEF) != 0 {
                    bad = Some((d, None));
                    break;
                }
                if let Some(t) = node
                    .pts
                    .to_sorted()
                    .into_iter()
                    .find(|t| *t != TOP && !r.members.contains(t) && !r.extra.contains(t))
                {
                    bad = Some((d, Some(t)));
                    break;
                }
            }
            match bad {
                Some((d, t)) => {
                    let what = match t {
                        Some(t) => self.tok_name(t),
                        None => format!("primitive {:#x}", self.nodes[d as usize].prim),
                    };
                    self.rfam_off(f, || format!("this {d} holds {what}"));
                    changed = true;
                }
                None => {
                    let r = self.rfam.fams.get_mut(&f).unwrap();
                    r.wseen = seen.split_off(r.dsts.len());
                    r.seen = seen;
                }
            }
        }
        changed
    }
}
