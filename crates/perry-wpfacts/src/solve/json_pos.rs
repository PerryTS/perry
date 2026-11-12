//! JSON tree positions (BATCHB26, fold BATCHB27). ECMA-262 25.5.1
//! JSON.parse makes every object and array of its result a fresh, distinct
//! object: no object of a parse sits at two positions until the program
//! aliases one. A JSON token stands for the objects at one POSITION CLASS
//! of the tree of one parse site. Each class has an object token and an
//! array token (`JsonPos::obj`, `JsonPos::arr`); a step is a name the
//! program reads (`P.k`), the wildcard under any other key (`P.*`, an
//! object's `field_any`) or the element class of an array (`A[]`, its
//! `field_any`).
//!
//! The children live in the parent's field nodes and are born when the
//! node is made (`field_r`, `field_any`), so every reader of the node sees
//! them. A JSON object's named fields read `named_any` (the values stored
//! under a key no store named), never its wildcard children: two named
//! children are distinct objects, while an unknown key reads all of them.
//! Symbol and internal keys birth no child: a JSON object has no own
//! property under them.
//!
//! FOLD: a class is keyed by its parse root and the LAST step of its path
//! (`Model::json_class`): every object whose path ends in the same step
//! shares the class's tokens, a merge of copies, never a dropped flow. The
//! set is finite: steps are the atoms the program names plus `*` and `[]`.
//! (Keyed by the last TWO steps, the three.js skin solve ran out of memory:
//! BATCHB27 m18.) Fields stay per class: a store into `j.a.x` is not read
//! from `j.b.x`, while `j.a.c` and `j.b.c` are one class.
//!
//! Default OFF (`FactsConfig::json_positions`, `PERRY_FACTS_JSON_POSITIONS=1`),
//! BATCHB28: on three.js skin an unknown-key store whose receiver and value
//! both hold the parse's classes fills every class's `named_any` with every
//! class, so every named field node holds the whole class set (field and
//! field-truthy-read nodes: 385M of 410M points-to entries at the bound).
//! A class merge on alias (an alias joins the classes, one token after) is
//! needed before the rule can be on.
use super::*;

impl Model {
    /// The root position class of one parse: object `obj`, array `arr`.
    pub(crate) fn json_root(&mut self, obj: Tok, arr: Tok) {
        let pos = JsonPos {
            root: obj,
            step: JsonStep::Root,
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
        let Some(&pos) = self.json_pos.get(&p) else {
            return;
        };
        let key = (pos.root, step);
        let (o, a) = match self.json_class.get(&key) {
            Some(&pair) => pair,
            None => self.json_birth(key),
        };
        self.add_tok(n, o);
        self.add_tok(n, a);
        self.add_prim(n, prim::ALL);
    }

    fn json_birth(&mut self, key: (Tok, JsonStep)) -> (Tok, Tok) {
        let (root, step) = key;
        let JsonPos { obj, arr, .. } = self.json_pos[&root];
        let (ctx, base) = match self.toks[root as usize] {
            TokKind::Obj(tm, c) => (c, self.tmpls[tm as usize].display.clone()),
            _ => (NO_CTX, String::new()),
        };
        let label = |m: &Model, s: JsonStep| match s {
            JsonStep::Named(a) => format!(".{}", m.atoms[a as usize]),
            JsonStep::Any => ".*".to_string(),
            JsonStep::Elem => "[]".to_string(),
            JsonStep::Root => String::new(),
        };
        let path = format!("{base}~{}", label(self, step));
        let o = self.json_clone(obj, ctx, path.clone());
        let a = self.json_clone(arr, ctx, format!("{path}:arr"));
        let pos = JsonPos {
            root,
            step,
            obj: o,
            arr: a,
        };
        self.json_pos.insert(o, pos);
        self.json_pos.insert(a, pos);
        self.json_class.insert(key, (o, a));
        self.rel(|| format!("jsonpos {o} {a} root={root}"));
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

    /// Diagnostic (`PERRY_FACTS_ABORT_TOP`): where the points-to entries sit
    /// when a solve stops on its bound.
    pub(crate) fn abort_top(&self) {
        let json = |t: Tok| self.json_pos.contains_key(&t);
        let mut lab: crate::pts::FxHashMap<&'static str, (u64, u64, u64)> = Default::default();
        let (mut tot, mut tj, mut nj) = (0u64, 0u64, 0u64);
        let mut big: Vec<(usize, NodeId)> = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            let l = n.pts.len();
            if l == 0 {
                continue;
            }
            let js = n.pts.to_sorted().iter().filter(|&&t| json(t)).count() as u64;
            tot += l as u64;
            tj += js;
            nj += (js > 0) as u64;
            let e = lab.entry(self.labels[i]).or_default();
            e.0 += l as u64;
            e.1 += js;
            e.2 += 1;
            big.push((l, i as NodeId));
        }
        eprintln!(
            "perry: wpfacts abort-top total={tot} json_entries={tj} nodes_with_json={nj} json_toks={}",
            self.json_pos.len()
        );
        let mut ls: Vec<_> = lab.into_iter().collect();
        ls.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        for (l, (s, j, c)) in ls.iter().take(15) {
            eprintln!("perry: wpfacts abort-label {l} sum={s} json={j} nodes={c}");
        }
        big.sort_unstable_by(|a, b| b.cmp(a));
        for (k, &(l, n)) in big.iter().take(40).enumerate() {
            let f = self.node_fn[n as usize];
            let fname = match f == NO_FN {
                true => "-".to_string(),
                false => self.funcs[f as usize].name.clone(),
            };
            let toks = self.nodes[n as usize].pts.to_sorted();
            let js = toks.iter().filter(|&&t| json(t)).count();
            let mut line = format!(
                "perry: wpfacts abort-node #{n} {} fn={fname} pts={l} json={js}",
                self.labels[n as usize]
            );
            if k < 5 {
                let other: Vec<String> = toks
                    .iter()
                    .filter(|&&t| !json(t))
                    .take(6)
                    .map(|&t| self.tok_name(t))
                    .collect();
                let jn: Vec<String> = toks
                    .iter()
                    .filter(|&&t| json(t))
                    .take(4)
                    .map(|&t| self.tok_name(t))
                    .collect();
                line += &format!(" other={other:?} jsonsome={jn:?}");
            }
            eprintln!("{}", line.chars().take(900).collect::<String>());
        }
    }
}
