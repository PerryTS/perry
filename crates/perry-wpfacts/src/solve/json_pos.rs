//! JSON tree positions (BATCHB26). ECMA-262 25.5.1 JSON.parse makes every
//! object and array of its result a fresh, distinct object: no object of a
//! parse sits at two positions until the program aliases one. A JSON token
//! stands for the objects at one POSITION CLASS of the tree of one parse
//! site: the root, a child under a name the program names (`P.k`), the
//! wildcard child under any other key (`P.*`, an object's `field_any`) or
//! the element class of an array (`A[]`, its `field_any`). Each class has
//! an object token and an array token (`JsonPos::obj`, `JsonPos::arr`).
//!
//! The children live in the parent's field nodes and are born when the
//! node is made (`field_r`, `field_any`), so every reader of the node sees
//! them. A JSON object's named fields read `named_any` (the values stored
//! under a key no store named), never its wildcard children: two named
//! children are distinct objects, while an unknown key reads all of them.
//!
//! FOLD (termination without a depth): a child `(P, s)` born while the
//! solver applies a constraint at receiver node `N` IS the nearest
//! ancestor-or-self `A` of `P` that was itself born with step `s` at `N`,
//! when there is one. Along a parent chain the `(site, step)` pairs are
//! then pairwise distinct, and both sets are finite: sites are the nodes
//! that exist when the solve starts (any other node counts as `NO_NODE`),
//! steps are atoms, `*` and `[]`. A recursive walker folds its path onto
//! itself at the read that recurses; access that does not recurse stays
//! exact.
use super::*;

impl Model {
    /// The root position class of one parse: object `obj`, array `arr`.
    pub(crate) fn json_root(&mut self, obj: Tok, arr: Tok) {
        let pos = JsonPos {
            parent: TOP,
            step: JsonStep::Root,
            site: NO_NODE,
            obj,
            arr,
        };
        self.json_pos.entry(obj).or_insert(pos);
        self.json_pos.entry(arr).or_insert(pos);
    }

    /// Is `t` the object token of a JSON position class?
    pub(crate) fn is_json_obj_pos(&self, t: Tok) -> bool {
        self.json_pos.get(&t).is_some_and(|p| p.obj == t)
    }

    /// Node `n` of JSON position `p` under `step` (a named field, the
    /// wildcard or the elements) holds the child class and any primitive.
    pub(crate) fn json_child(&mut self, p: Tok, step: JsonStep, n: NodeId) {
        if !self.json_pos.contains_key(&p) {
            return;
        }
        let site = match self.json_site < self.json_site_limit {
            true => self.json_site,
            false => NO_NODE,
        };
        let (o, a) = match self.json_fold(p, step, site) {
            Some(pair) => pair,
            None => self.json_birth(p, step, site),
        };
        self.add_tok(n, o);
        self.add_tok(n, a);
        self.add_prim(n, prim::ALL);
    }

    fn json_fold(&self, p: Tok, step: JsonStep, site: NodeId) -> Option<(Tok, Tok)> {
        let mut at = p;
        loop {
            let pos = self.json_pos[&at];
            if pos.step == step && pos.site == site {
                return Some((pos.obj, pos.arr));
            }
            if pos.step == JsonStep::Root {
                return None;
            }
            at = pos.parent;
        }
    }

    fn json_birth(&mut self, p: Tok, step: JsonStep, site: NodeId) -> (Tok, Tok) {
        let mut root = p;
        while self.json_pos[&root].step != JsonStep::Root {
            root = self.json_pos[&root].parent;
        }
        let JsonPos { obj, arr, .. } = self.json_pos[&root];
        let (ctx, path) = match self.toks[p as usize] {
            TokKind::Obj(tm, c) => (c, self.tmpls[tm as usize].display.clone()),
            _ => (NO_CTX, String::new()),
        };
        let path = match step {
            JsonStep::Named(a) => format!("{path}.{}", self.atoms[a as usize]),
            JsonStep::Any => format!("{path}.*"),
            JsonStep::Elem => format!("{path}[]"),
            JsonStep::Root => path,
        };
        let o = self.json_clone(obj, ctx, path.clone());
        let a = self.json_clone(arr, ctx, format!("{path}:arr"));
        let pos = JsonPos {
            parent: p,
            step,
            site,
            obj: o,
            arr: a,
        };
        self.json_pos.insert(o, pos);
        self.json_pos.insert(a, pos);
        self.rel(|| format!("jsonpos {o} {a} parent={p} site={site}"));
        (o, a)
    }

    fn json_clone(&mut self, like: Tok, ctx: CallId, display: String) -> Tok {
        let TokKind::Obj(tm, _) = self.toks[like as usize] else {
            unreachable!("a JSON root is an object token")
        };
        let mut t = self.tmpls[tm as usize].clone();
        t.display = display;
        let ntm = self.tmpls.len() as TmplId;
        self.tmpls.push(t);
        self.tok(TokKind::Obj(ntm, ctx))
    }
}
