#!/usr/bin/env python3
"""wpfacts independent recomputation (design 6.6, checker 1).

Re-solves the constraint system the compiler generated, with a separate,
plain implementation of the solver rules (design 3.2), and re-derives the
facts F1, F1', F2, F3, F6 and the per-site verdicts (F4/F5). Then diffs
them against the compiler's facts dump, in both directions:

  soundness  the compiler claims a fact the recomputation does not;
  precision  the recomputation proves something the compiler does not.

Inputs (written by `perry compile` with PERRY_FACTS_DUMP=<f> and
PERRY_FACTS_RELATIONS=1): <f> (facts dump) and <f>.rel (relations).

Relation tags: G = generation (inputs), B = builtin summaries (axioms),
S = the compiler's own rule applications, M = metadata.

Method: the recomputation takes every relation the compiler recorded as an
axiom (they can only add facts) AND applies its own, separately written
implementation of the solver rules to everything that reaches every node,
until a plain fixpoint. Its points-to sets therefore contain the
compiler's; anything EXTRA it finds is a token the compiler's solver or
rules missed, which shows up below as a soundness diff. The derivation of
the facts from the points-to sets is re-implemented independently too.

Usage: wpfacts_recompute.py <facts-dump> [--quiet]
Exit 0 when the diff is empty, 1 otherwise.
"""
import sys
from collections import defaultdict, deque

TOP = 0
NUM, STR, BOOL, NULL, UNDEF, BIGINT, SYMBOL = 1, 2, 4, 8, 16, 32, 64
ALL = 127
PRIM_NAMES = [(NUM, "num"), (STR, "str"), (BOOL, "bool"), (NULL, "null"),
              (UNDEF, "undef"), (BIGINT, "bigint"), (SYMBOL, "symbol")]
SUMMARIZED_NATIVE = {"Error", "TypeError", "RangeError", "SyntaxError",
                     "ReferenceError", "EventEmitter"}
STAR = "\x01*"


def unesc(s):
    """Inverse of perry-wpfacts `model::esc_key`."""
    if s == "\\e":
        return ""
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            n = s[i + 1]
            if n == "u" and s[i + 2:i + 3] == "{":
                j = s.index("}", i)
                out.append(chr(int(s[i + 3:j], 16)))
                i = j + 1
                continue
            out.append({"\\": "\\", "s": " ", "n": "\n", "r": "\r", "t": "\t"}.get(n, n))
            i += 2
            continue
        out.append(c)
        i += 1
    return "".join(out)


def esc(s):
    """perry-wpfacts `model::esc_key`."""
    if s == "":
        return "\\e"
    out = []
    for c in s:
        if c == "\\":
            out.append("\\\\")
        elif c == "*":
            out.append("\\*")
        elif c == " ":
            out.append("\\s")
        elif c == "\n":
            out.append("\\n")
        elif c == "\r":
            out.append("\\r")
        elif c == "\t":
            out.append("\\t")
        elif c.isspace() or (ord(c) < 32 or 0x7f <= ord(c) < 0xa0):
            out.append("\\u{%x}" % ord(c))
        else:
            out.append(c)
    return "".join(out)


class Solver:
    def __init__(self):
        self.pts = defaultdict(set)
        self.prim = defaultdict(int)
        self.succ = defaultdict(set)
        self.cons = defaultdict(list)
        self.wl = deque()
        self.inwl = set()
        self.next_node = 1 << 40  # our own nodes, disjoint from the compiler's
        # metadata
        self.atoms = {}
        self.atom_id = {}
        self.toks = {}          # id -> (kind, arg)
        self.tok_of = {}        # (kind, arg) -> id
        self.tmpls = {}         # id -> dict
        self.funcs = {}         # id -> dict
        self.func_by_tok = {}
        self.fn_birth = {}
        self.calls = {}
        self.stores = {}
        self.bound = {}
        self.reasons = {}
        self.thrown = None
        self.toptok = None
        # derived structures
        self.fields = {}
        self.fieldany = {}
        self.allfields = {}
        self.protonode = {}
        self.field_keys = defaultdict(list)
        self.accessors = defaultdict(lambda: ([], []))
        self.acc_keys = defaultdict(list)
        self.visits = defaultdict(set)
        self.writers = defaultdict(lambda: [set(), set()])
        self.any_writers = defaultdict(lambda: [set(), set()])
        self.adds = defaultdict(int)
        self.escaped = set()
        self.hard = {}
        self.dyn_eval = None
        self.done = set()
        self.next_call = 1 << 40
        self.read_sites = []
        self.objproto_accessor = False
        self.top_calls = 0
        self.interp = None
        self.whatif = False
        self.stopped = defaultdict(list)
        self.widened = set()
        self.escape_queue = []
        self.cv_nodes = {}
        self.cp_nodes = {}
        self.stopped_shared = defaultdict(list)
        # naming declared by the compiler (fieldnode … lines)
        self.named_field = {}
        self.named_fieldany = {}
        self.named_allfields = {}
        self.named_protonode = {}

    # ------------------------------------------------------------ basics
    def node(self):
        self.next_node += 1
        return self.next_node

    def push(self, n):
        if n not in self.inwl:
            self.inwl.add(n)
            self.wl.append(n)

    def add(self, n, toks=(), prim=0):
        changed = False
        if n in self.widened:
            # The compiler widened this set: its members escape, it holds ⊤.
            for t in toks:
                if t != TOP:
                    self.escape_queue.append(t)
            toks = [TOP]
        s = self.pts[n]
        new = [t for t in toks if t not in s]
        if new:
            s.update(new)
            changed = True
        np_ = prim & ~self.prim[n]
        if np_:
            self.prim[n] |= np_
            changed = True
        if changed:
            self.pending.setdefault(n, [set(), 0])
            self.pending[n][0].update(new)
            self.pending[n][1] |= np_
            self.push(n)

    def edge(self, a, b):
        if a == b or b in self.succ[a]:
            return
        self.succ[a].add(b)
        self.add(b, list(self.pts[a]), self.prim[a])

    def addcons(self, n, c):
        key = (n,) + c
        if key in self.done:
            return
        self.done.add(key)
        self.cons[n].append(c)
        if self.pts[n] or self.prim[n]:
            self.apply(n, c, list(self.pts[n]), self.prim[n])

    def tok(self, kind, arg):
        k = (kind, arg)
        if k in self.tok_of:
            return self.tok_of[k]
        t = max(self.toks) + 1 if self.toks else 1
        self.toks[t] = k
        self.tok_of[k] = t
        return t

    def atom(self, s):
        if s in self.atom_id:
            return self.atom_id[s]
        a = max(self.atoms) + 1 if self.atoms else 0
        self.atoms[a] = s
        self.atom_id[s] = a
        return a

    def tmpl_of(self, t):
        k = self.toks.get(t)
        return k[1] if k and k[0] == "obj" else None

    def is_fact_unit(self, tm):
        return self.tmpls[tm]["kind"] in ("class", "lit", "fninst")

    # ------------------------------------------------------------ fields
    def field(self, t, a):
        k = (t, a)
        if k in self.fields:
            return self.fields[k]
        n = self.named_field.get(k) or self.node()
        self.fields[k] = n
        self.field_keys[t].append(a)
        self.edge(self.fany(t), n)
        self.edge(n, self.fall(t))
        return n

    def fany(self, t):
        if t in self.fieldany:
            return self.fieldany[t]
        n = self.named_fieldany.get(t) or self.node()
        self.fieldany[t] = n
        self.edge(n, self.fall(t))
        return n

    def fall(self, t):
        if t in self.allfields:
            return self.allfields[t]
        n = self.named_allfields.get(t) or self.node()
        self.allfields[t] = n
        return n

    def pnode(self, t):
        if t in self.protonode:
            return self.protonode[t]
        n = self.named_protonode.get(t) or self.node()
        self.protonode[t] = n
        op = self.tok("objectproto", "-")
        kind, arg = self.toks[t]
        if kind == "obj":
            tk = self.tmpls[arg]["kind"]
            if tk == "class":
                self.add(n, [self.tok("proto", arg)])
            elif tk == "fninst":
                f = self.tmpls[arg]["func"]
                ftok = self.funcs[f]["tok"]
                self.edge(self.field(ftok, self.atom("prototype")), n)
            else:
                self.add(n, [op])
        elif kind == "proto":
            tp = self.tmpls[arg]
            if tp["parent"] is not None:
                self.add(n, [self.tok("proto", tp["parent"])])
            elif tp["native"] is not None:
                self.add(n, [self.tok("nativeproto", tp["native"])])
            else:
                self.add(n, [op])
        elif kind == "nativeproto":
            self.add(n, [op])
        elif kind in ("func", "bound", "builtinfn"):
            self.add(n, [op])
        elif kind == "classobj":
            tp = self.tmpls[arg]
            if tp["parent"] is not None:
                self.add(n, [self.tok("classobj", tp["parent"])])
            else:
                self.add(n, [op])
        return n

    def sink(self):
        n = self.node()
        self.addcons(n, ("top",))
        return n

    def fninst(self, f):
        for tm, tp in self.tmpls.items():
            if tp["kind"] == "fninst" and tp["func"] == f:
                return tm
        tm = max(self.tmpls) + 1
        b, d = self.fn_birth.get(f, ([], []))
        self.tmpls[tm] = dict(kind="fninst", func=f, birth=list(b), definite=list(d),
                              parent=None, ctor=f, native=None, acc=[], computed=0,
                              fieldinit=None, display="new " + self.funcs[f].get("name", str(f)),
                              class_id=None)
        return tm

    def new_call(self, this, args, spread, ret):
        self.next_call += 1
        self.calls[self.next_call] = dict(this=this, args=list(args), spread=list(spread), ret=ret)
        return self.next_call

    # ------------------------------------------------------------- rules
    def apply(self, n, c, toks, prim):
        op = c[0]
        if op == "load":
            _, key, dst = c
            for t in toks:
                self.load_from(t, t, key, dst)
        elif op == "store":
            _, key, src, site = c
            for t in toks:
                self.store_into(t, key, src, site)
        elif op == "call":
            for t in toks:
                self.call_tok(c[1], t, None)
        elif op == "callthis":
            for t in toks:
                self.call_tok(c[1], t, c[2])
        elif op == "new":
            for t in toks:
                self.construct(c[1], t)
        elif op == "mcall":
            _, key, call = c
            for t in toks:
                self.method_on(t, key, call)
        elif op == "chain":
            _, origin, key, act = c
            for p in toks:
                self.chain_step(origin, p, key, act)
        elif op == "top":
            for t in toks:
                self.escape(t)
        elif op == "hard":
            for t in toks:
                self.mark_hard(t, c[1])
        elif op == "addskeys":
            for t in toks:
                tm = self.tmpl_of(t)
                if tm is not None:
                    self.adds[tm] += 1
        elif op == "implicit":
            for t in toks:
                if self.toks[t][0] in ("func", "bound"):
                    self.escape(t)
                else:
                    self.implicit(t)
        elif op == "createproto":
            for t in toks:
                k = self.toks[t]
                if k[0] == "proto":
                    self.mark_hard(self.tok("obj", k[1]), "E10")
        elif op == "super":
            _, start, key, act = c
            for t in toks:
                self.chain_step(t, start, key, act)
        elif op == "superctor":
            _, cls, call = c
            for t in toks:
                if t == TOP:
                    continue
                p = self.tmpls[cls]["parent"]
                if p is not None:
                    self.run_ctor_chain(call, p, t)
        elif op == "thisfrom":
            recv = c[1]
            for t in toks:
                k = self.toks[t]
                if k[0] == "func" and not self.funcs[k[1]]["lexical"]:
                    self.edge(recv, self.funcs[k[1]]["this"])
        elif op == "accessor":
            _, holder, key, setter = c
            for t in toks:
                k = self.toks[t]
                if k[0] == "func":
                    self.addcons(holder, ("accinstall", key, k[1], setter))
                elif k[0] in ("top", "builtinfn", "bound"):
                    fa = self.node()
                    self.add(fa, [TOP])
                    self.addcons(holder, ("store", "*", fa, -1))
                    self.addcons(holder, ("top",))
        elif op == "accinstall":
            _, key, f, setter = c
            for t in toks:
                if t == TOP:
                    continue
                if self.toks[t][0] == "objectproto":
                    self.objproto_accessor = True
                    continue
                if self.atoms.get(key) == STAR:
                    # computed-key accessor: runs as unknown code
                    self.escape(self.funcs[f]["tok"])
                    self.add(self.fany(t), [TOP], ALL)
                    if key not in self.acc_keys[t]:
                        self.acc_keys[t].append(key)
                    continue
                lst = self.accessors[(t, key)][1 if setter else 0]
                if f in lst:
                    continue
                lst.append(f)
                if key not in self.acc_keys[t]:
                    self.acc_keys[t].append(key)
                for (origin, k2, act) in list(self.visits[t]):
                    self.accessor_hit(origin, t, k2, act)
        elif op == "protofrom":
            for t in toks:
                if t != TOP:
                    self.edge(c[1], self.pnode(t))
        elif op == "protoof":
            for t in toks:
                if t == TOP:
                    self.add(c[1], [TOP])
                else:
                    self.edge(self.pnode(t), c[1])
        elif op == "iternext":
            dst = c[1]
            for t in toks:
                k = self.toks[t]
                if k[0] == "top":
                    self.add(dst, [TOP])
                elif k[0] in ("func", "bound"):
                    self.add(dst, [t])
                elif k[0] == "obj" and self.tmpls[k[1]]["kind"] in ("arr", "bobj"):
                    pass
                elif k[0] == "primbox":
                    pass
                else:
                    self.load_from(t, t, self.atom("next"), dst)
        elif op == "newtargethard":
            for t in toks:
                k = self.toks[t]
                if k[0] == "classobj":
                    self.mark_hard(self.tok("obj", k[1]), "E11")
                elif k[0] == "func":
                    self.mark_hard(self.tok("obj", self.fninst(k[1])), "E11")
        elif op == "arith":
            _, dst, add = c
            out = 0
            if toks:
                out |= NUM | BIGINT | (STR if add else 0)
            if prim & BIGINT:
                out |= BIGINT
            if prim & (NUM | BOOL | NULL | UNDEF):
                out |= NUM
            if prim & STR:
                out |= STR if add else NUM
            if out:
                self.add(dst, [], out)
        elif op == "idxload":
            _, idx, dst = c
            for t in toks:
                if t == TOP:
                    self.add(dst, [TOP]); continue
                self.edge(self.fall(t), dst)
                self.add(dst, [], UNDEF)
                self.addcons(idx, ("idxkey", t, dst))
        elif op == "idxkey":
            _, obj, dst = c
            if (prim & (STR | SYMBOL)) or toks:
                key = ("idxfull", obj, dst)
                if key not in self.done:
                    self.done.add(key)
                    self.load_from(obj, obj, "*", dst)
        elif op == "chainunion":
            _, key, dst = c
            for q in toks:
                if q == TOP:
                    self.add(dst, [TOP])
                    if key == "*" or self.atoms.get(key) == "constructor":
                        self.add(dst, [self.tok("builtin", "Function")])
                    continue
                self.edge(self.fall(q) if key == "*" else self.field(q, key), dst)
                k = self.toks[q]
                if (key != "*" and k[0] == "proto" and key in self.tmpls[k[1]]["pm"]
                        and q not in self.hard and q not in self.escaped):
                    self.stopped_shared[q].append((key, dst))
                else:
                    self.edge(self.cv(q, key), dst)
        elif op == "protoclosure":
            dst = c[1]
            for q in toks:
                self.add(dst, [q])
                if q != TOP:
                    self.edge(self.cp(q), dst)
        elif op == "anyaccessors":
            _, origin, act = c
            for q in toks:
                if q != TOP and self.acc_keys.get(q):
                    self.accessor_hit(origin, q, "*", act)
        elif op == "protomethods":
            pass  # builtin prototype methods: their effects are B axioms
        elif op == "storetopproto":
            _, origin, src = c
            if TOP in toks:
                self.edge(src, self.sink())
                self.escape(origin)
        elif op == "sharebuffer":
            ta = c[1]
            for t in toks:
                k = self.toks[t]
                if t == TOP or (k[0] == "obj" and self.tmpls[k[1]]["kind"] == "bobj"
                                and self.tmpls[k[1]]["what"] == "TypedArray"):
                    self.mark_hard(ta, "B1")

    def present(self, t, key):
        tm = self.tmpl_of(t)
        if key == "*" or tm is None:
            return False
        return self.is_fact_unit(tm) and key in self.tmpls[tm]["definite"]

    def load_from(self, origin, t, key, dst):
        if t == TOP:
            self.add(dst, [TOP])
            if key == "*" or self.atoms.get(key) == "constructor":
                self.add(dst, [self.tok("builtin", "Function")])
            return
        if not self.present(t, key):
            self.add(dst, [], UNDEF)
        if key == "*":
            self.edge(self.fall(t), dst)
        else:
            self.edge(self.field(t, key), dst)
        if key == "*" and self.is_array_like(t):
            return
        self.chain_from(origin, t, key, ("load", dst))

    def is_array_like(self, t):
        k = self.toks[t]
        return k[0] == "primbox" or (k[0] == "obj" and self.tmpls[k[1]]["kind"] in ("arr", "bobj"))

    def chain_from(self, origin, t, key, act):
        if self.toks[t][0] != "objectproto":
            self.visits[t].add((origin, key, act))
        self.accessor_hit(origin, t, key, act)
        self.addcons(self.pnode(t), ("chain", origin, key, act))

    def chain_step(self, origin, p, key, act):
        if p == TOP:
            if act[0] == "load":
                self.add(act[1], [TOP])
            elif act[0] == "call":
                self.call_tok(act[1], TOP, origin)
            else:
                self.edge(act[1], self.sink())
            return
        if act[0] == "load":
            self.edge(self.fall(p) if key == "*" else self.field(p, key), act[1])
        elif act[0] == "call":
            callee = self.fall(p) if key == "*" else self.field(p, key)
            self.bind_via(callee, act[1], origin)
        k = self.toks[p]
        if (key != "*" and k[0] == "proto" and key in self.tmpls[k[1]]["pm"]
                and p not in self.hard and p not in self.escaped):
            # The declared member ends the lookup, but a declared accessor
            # there still runs (a setter/getter reached by the lookup).
            self.accessor_hit(origin, p, key, act)
            self.stopped[p].append((origin, key, act))
            return
        self.chain_from(origin, p, key, act)

    def resume(self, p):
        for (origin, key, act) in self.stopped.pop(p, []):
            self.chain_from(origin, p, key, act)
        for (key, dst) in self.stopped_shared.pop(p, []):
            self.edge(self.cv(p, key), dst)

    def cv(self, t, key):
        k = (t, key)
        if k not in self.cv_nodes:
            n = self.node()
            self.cv_nodes[k] = n
            self.addcons(self.pnode(t), ("chainunion", key, n))
        return self.cv_nodes[k]

    def cp(self, t):
        if t not in self.cp_nodes:
            n = self.node()
            self.cp_nodes[t] = n
            self.addcons(self.pnode(t), ("protoclosure", n))
        return self.cp_nodes[t]

    def accessor_hit(self, origin, holder, key, act):
        if key == "*":
            keys = list(self.acc_keys.get(holder, []))
        else:
            keys = [key]
        for a in keys:
            if (holder, a) not in self.accessors:
                continue
            getters, setters = self.accessors[(holder, a)]
            if act[0] == "load":
                for g in getters:
                    self.add(self.funcs[g]["this"], [origin])
                    self.edge(self.funcs[g]["ret"], act[1])
            elif act[0] == "store":
                for s in setters:
                    self.add(self.funcs[s]["this"], [origin])
                    if self.funcs[s]["params"]:
                        self.edge(act[1], self.funcs[s]["params"][0])
            else:
                for g in getters:
                    self.add(self.funcs[g]["this"], [origin])
                    self.bind_via(self.funcs[g]["ret"], act[1], origin)

    def bind_via(self, callee, call, origin):
        self.addcons(callee, ("callthis", call, origin))

    def store_into(self, t, key, src, site):
        if t == TOP:
            self.edge(src, self.sink())
            return
        tm = self.tmpl_of(t)
        st = self.stores.get(site, dict(birth=1, ctor_phase_of=None, ctor_fn=None))
        if key != "*":
            self.edge(src, self.field(t, key))
            if tm is not None:
                ctor = st["birth"] or self.is_ctor_phase(st, tm)
                w = self.writers[(tm, key)]
                (w[0] if ctor else w[1]).add(site)
                if key not in self.tmpls[tm]["birth"] and not ctor:
                    self.adds[tm] += 1
        else:
            self.edge(src, self.fany(t))
            if tm is not None:
                w = self.any_writers[tm]
                if st["birth"]:
                    w[0].add(site)
                else:
                    w[1].add(site)
                    self.adds[tm] += 1
        self.chain_from(t, t, key, ("store", src))

    def is_ctor_phase(self, st, tm):
        tp = self.tmpls[tm]
        if tp["kind"] == "fninst":
            return st["ctor_fn"] == tp["func"]
        owner = st["ctor_phase_of"]
        if owner is None:
            return False
        cur = tm
        while cur is not None:
            if cur == owner:
                return True
            cur = self.tmpls[cur]["parent"]
        return False

    def call_tok(self, call, t, this_tok):
        key = ("bind", call, t, this_tok)
        if key in self.done:
            return
        self.done.add(key)
        c = self.calls[call]
        kind, arg = self.toks[t]
        if kind in ("top", "builtinfn"):
            if kind == "top":
                self.top_calls += 1
            s = self.sink()
            for a in c["args"] + c["spread"]:
                self.edge(a, s)
            th = c["this"]
            if this_tok is not None:
                self.add(s, [this_tok])
            elif th[0] == "n":
                self.edge(th[1], s)
            elif th[0] == "t":
                self.add(s, [th[1]])
            self.add(c["ret"], [TOP], ALL)
        elif kind == "func":
            fi = self.funcs[arg]
            for i, a in enumerate(c["args"]):
                if i < len(fi["params"]):
                    self.edge(a, fi["params"][i])
                elif fi["rest"] is not None:
                    self.edge(a, self.elems_into(fi["rest"]))
                self.edge(a, fi["args"])
            for p in fi["params"][len(c["args"]):]:
                self.add(p, [], UNDEF)
            for s in c["spread"]:
                el = self.node()
                self.addcons(s, ("load", "*", el))
                for p in fi["params"]:
                    self.edge(el, p)
                if fi["rest"] is not None:
                    self.edge(el, self.elems_into(fi["rest"]))
                self.edge(el, fi["args"])
            if not fi["lexical"]:
                if this_tok is not None:
                    self.add(fi["this"], [this_tok])
                else:
                    th = c["this"]
                    if th[0] == "t":
                        self.add(fi["this"], [th[1]])
                    elif th[0] == "n":
                        self.edge(th[1], fi["this"])
                    else:
                        self.add(fi["this"], [], UNDEF)
            self.edge(fi["ret"], c["ret"])
        elif kind == "bound":
            target, bthis, bargs = self.bound[arg]
            allargs = self.node()
            for a in bargs + c["args"] + c["spread"]:
                self.edge(a, allargs)
            self.edge(allargs, self.fany(t))
            arr = self.node()
            self.add(arr, [t])
            c2 = self.new_call(("n", bthis), bargs + c["args"], c["spread"] + [arr], c["ret"])
            self.addcons(target, ("call", c2))

    def elems_into(self, arr_node):
        relay = self.node()
        self.addcons(arr_node, ("store", "*", relay, -1))
        return relay

    def construct(self, call, t):
        key = ("construct", call, t)
        if key in self.done:
            return
        self.done.add(key)
        c = self.calls[call]
        kind, arg = self.toks[t]
        if kind == "classobj":
            inst = self.tok("obj", arg)
            self.add(c["ret"], [inst])
            self.run_ctor_chain(call, arg, inst)
        elif kind == "func":
            tm = self.fninst(arg)
            inst = self.tok("obj", tm)
            self.add(c["ret"], [inst])
            self.call_tok(call, t, inst)
            self.edge(self.funcs[arg]["ret"], c["ret"])
        elif kind in ("top", "builtinfn"):
            if kind == "top":
                self.top_calls += 1
            s = self.sink()
            for a in c["args"] + c["spread"]:
                self.edge(a, s)
            self.add(c["ret"], [TOP])
        elif kind == "bound":
            target, bthis, bargs = self.bound[arg]
            c2 = self.new_call(("u",), bargs + c["args"], c["spread"], c["ret"])
            self.addcons(target, ("new", c2))

    def run_ctor_chain(self, call, tm, inst):
        c = self.calls[call]
        tp = self.tmpls[tm]
        if tp["fieldinit"] is not None:
            self.add(self.funcs[tp["fieldinit"]]["this"], [inst])
        if tp["ctor"] is not None:
            f = tp["ctor"]
            self.call_tok(call, self.funcs[f]["tok"], inst)
            self.edge(self.funcs[f]["ret"], c["ret"])
        elif tp["parent"] is not None:
            self.run_ctor_chain(call, tp["parent"], inst)

    def method_on(self, t, key, call):
        if t == TOP:
            self.call_tok(call, TOP, TOP)
            return
        own = self.fall(t) if key == "*" else self.field(t, key)
        self.bind_via(own, call, t)
        self.chain_from(t, t, key, ("call", call))

    def escape(self, t):
        if t == TOP or t in self.escaped:
            return
        if self.toks[t][0] in ("objectproto", "builtinfn", "builtin", "nativeproto"):
            return
        self.escaped.add(t)
        if self.toks[t][0] == "proto":
            self.resume(t)
        self.addcons(self.fall(t), ("top",))
        self.add(self.fany(t), [TOP], ALL)
        tm = self.tmpl_of(t)
        if tm is not None:
            self.adds[tm] += 1
        self.addcons(self.pnode(t), ("top",))
        self.implicit(t)
        kind, arg = self.toks[t]
        if kind == "func":
            fi = self.funcs[arg]
            for p in fi["params"] + ([fi["rest"]] if fi["rest"] is not None else []):
                self.add(p, [TOP], ALL)
            if fi["rest"] is not None:
                self.add(self.elems_into(fi["rest"]), [TOP])
            self.add(fi["args"], [TOP])
            if not fi["lexical"]:
                self.add(fi["this"], [TOP], ALL)
            self.edge(fi["ret"], self.sink())
        elif kind == "classobj":
            self.escape(self.tok("proto", arg))
            ret = self.node()
            an = self.node()
            self.add(an, [TOP], ALL)
            call = self.new_call(("u",), [an] * 8, [], ret)
            self.construct(call, t)
            self.edge(ret, self.sink())
            self.escape(self.tok("obj", arg))
        elif kind == "obj":
            tk = self.tmpls[arg]["kind"]
            if tk == "class":
                self.escape(self.tok("classobj", arg))
            if tk == "fninst":
                self.escape(self.funcs[self.tmpls[arg]["func"]]["tok"])
        elif kind == "proto":
            if self.tmpls[arg]["kind"] == "class":
                self.escape(self.tok("classobj", arg))
        elif kind == "bound":
            target, bthis, bargs = self.bound[arg]
            for n in [target, bthis] + bargs:
                self.edge(n, self.sink())

    def mark_hard(self, t, code):
        if t == TOP or t in self.hard:
            return
        self.hard[t] = code
        if self.toks[t][0] == "proto":
            self.resume(t)
        self.add(self.fany(t), [], ALL)
        tm = self.tmpl_of(t)
        if tm is not None:
            self.adds[tm] += 1

    def implicit(self, t):
        key = ("implicit", t)
        if t == TOP or key in self.done:
            return
        self.done.add(key)
        ret = self.node()
        arg = self.node()
        self.add(arg, [], ALL)
        call = self.new_call(("t", t), [arg] * 4, [], ret)
        self.method_on(t, "*", call)
        self.addcons(ret, ("implicit",))

    def solve(self):
        while self.wl or self.escape_queue:
            while self.escape_queue:
                self.escape(self.escape_queue.pop())
            if not self.wl:
                continue
            n = self.wl.popleft()
            self.inwl.discard(n)
            pend = self.pending.pop(n, None)
            if not pend:
                continue
            toks, prim = list(pend[0]), pend[1]
            for s in list(self.succ[n]):
                self.add(s, toks, prim)
            for c in list(self.cons[n]):
                self.apply(n, c, toks, prim)


def key_of(s, name):
    return "*" if name == "*" else s.atom(unesc(name))


def parse_act(s, txt):
    # Rust Debug: Load(12) | Call(3) | Store(9)
    kind, num = txt.rstrip(")").split("(")
    return (kind.lower(), int(num))


def load(path):
    s = Solver()
    s.pending = {}
    rel_lines = []
    with open(path) as f:
        for line in f:
            line = line.rstrip("\n")
            if not line or line.startswith("#") or line.startswith("tokname"):
                continue
            if line.startswith("M "):
                p = line.split(" ")
                what = p[1]
                if what == "atom":
                    a = int(p[2]); nm = unesc(" ".join(p[3:]))
                    s.atoms[a] = nm; s.atom_id.setdefault(nm, a)
                elif what == "reason":
                    s.reasons[int(p[2])] = " ".join(p[3:])
                elif what == "tok":
                    t = int(p[2]); k = (p[3], p[4] if len(p) > 4 else "-")
                    if k[0] in ("obj", "func", "bound", "classobj", "proto", "modns"):
                        k = (k[0], int(k[1]))
                    s.toks[t] = k; s.tok_of[k] = t
                elif what == "tmpl":
                    tm = int(p[2]); kind = p[3]; arg = p[4]
                    kv = dict(x.split("=", 1) for x in p[5:] if "=" in x)
                    ints = lambda v: [] if v == "-" else [int(x) for x in v.split(",")]
                    opt = lambda v: None if v == "-" else int(v)
                    d = dict(kind=kind, birth=ints(kv["birth"]), definite=ints(kv["definite"]),
                             parent=opt(kv["parent"]), ctor=opt(kv["ctor"]),
                             native=None if kv["native"] == "-" else kv["native"],
                             acc=ints(kv["acc"]), computed=int(kv["computed"]),
                             fieldinit=opt(kv["fieldinit"]), display=unesc(kv["display"]),
                             class_id=int(arg) if kind == "class" else None,
                             func=int(arg) if kind == "fninst" else None,
                             what=arg if kind == "bobj" else None,
                             pm=ints(kv.get("pm", "-")))
                    s.tmpls[tm] = d
                elif what == "func":
                    f = int(p[2])
                    kv = dict(x.split("=", 1) for x in p[3:])
                    ints = lambda v: [] if v == "-" else [int(x) for x in v.split(",")]
                    s.funcs[f] = dict(tok=int(kv["tok"]), this=int(kv["this"]), ret=int(kv["ret"]),
                                      args=int(kv["args"]),
                                      rest=None if kv["rest"] == "-" else int(kv["rest"]),
                                      lexical=kv["lexical"] == "1", params=ints(kv["params"]))
                    s.func_by_tok[int(kv["tok"])] = f
                elif what == "fnbirth":
                    kv = dict(x.split("=", 1) for x in p[3:])
                    ints = lambda v: [] if v == "-" else [int(x) for x in v.split(",")]
                    s.fn_birth[int(p[2])] = (ints(kv["birth"]), ints(kv["definite"]))
                elif what == "call":
                    c = int(p[2])
                    kv = dict(x.split("=", 1) for x in p[3:])
                    ints = lambda v: [] if v == "-" else [int(x) for x in v.split(",")]
                    th = kv["this"]
                    this = ("u",) if th == "u" else (th[0], int(th[1:]))
                    s.calls[c] = dict(this=this, args=ints(kv["args"]), spread=ints(kv["spread"]),
                                      ret=int(kv["ret"]))
                elif what == "store":
                    st = int(p[2])
                    kv = dict(x.split("=", 1) for x in p[3:])
                    opt = lambda v: None if v == "-" else int(v)
                    s.stores[st] = dict(birth=kv["birth"] == "1", ctor_phase_of=opt(kv["ctor_phase_of"]),
                                        ctor_fn=opt(kv["ctor_fn"]))
                elif what == "bound":
                    b = int(p[2])
                    kv = dict(x.split("=", 1) for x in p[3:])
                    ints = lambda v: [] if v == "-" else [int(x) for x in v.split(",")]
                    s.bound[b] = (int(kv["target"]), int(kv["this"]), ints(kv["args"]))
                elif what == "special":
                    kv = dict(x.split("=", 1) for x in p[2:])
                    s.thrown = int(kv["thrown"]); s.toptok = int(kv["toptok"])
                continue
            rel_lines.append(line)
    for fx, fi in s.funcs.items():
        fi["name"] = str(fx)
    return s, rel_lines


def run(s, rel_lines):
    # Naming first (any phase), then the inputs (G, B).
    for line in rel_lines:
        p = line.split(" ")
        rel = p[1]
        if rel == "widen":
            s.widened.add(int(p[2]))
        elif rel == "fieldnode":
            s.named_field[(int(p[3]), int(p[4]))] = int(p[2])
        elif rel == "fieldany":
            s.named_fieldany[int(p[3])] = int(p[2])
        elif rel == "allfields":
            s.named_allfields[int(p[3])] = int(p[2])
        elif rel == "protonode":
            s.named_protonode[int(p[3])] = int(p[2])
    deferred = []
    for line in rel_lines:
        p = line.split(" ")
        tag, rel = p[0], p[1]
        a = p[2:]
        if rel == "addr":
            s.add(int(a[0]), [int(a[1])])
        elif rel == "prim":
            s.add(int(a[0]), [], int(a[1]))
        elif rel == "copy":
            s.edge(int(a[0]), int(a[1]))
        elif rel == "load":
            deferred.append((int(a[0]), ("load", key_of(s, a[1]), int(a[2]))))
        elif rel == "store":
            deferred.append((int(a[0]), ("store", key_of(s, a[1]), int(a[2]), int(a[3]))))
        elif rel in ("call", "new"):
            deferred.append((int(a[0]), (rel, int(a[1]))))
        elif rel == "callthis":
            deferred.append((int(a[0]), ("callthis", int(a[1]), int(a[2]))))
        elif rel == "mcall":
            deferred.append((int(a[0]), ("mcall", key_of(s, a[1]), int(a[2]))))
        elif rel == "top":
            deferred.append((int(a[0]), ("top",)))
        elif rel == "hard":
            deferred.append((int(a[0]), ("hard", a[1])))
        elif rel == "addskeys":
            deferred.append((int(a[0]), ("addskeys",)))
        elif rel == "implicit":
            deferred.append((int(a[0]), ("implicit",)))
        elif rel == "createproto":
            deferred.append((int(a[0]), ("createproto",)))
        elif rel == "super":
            deferred.append((int(a[0]), ("super", int(a[1]), key_of(s, a[2]), parse_act(s, a[3]))))
        elif rel == "superctor":
            deferred.append((int(a[0]), ("superctor", int(a[1]), int(a[2]))))
        elif rel == "thisfrom":
            deferred.append((int(a[0]), ("thisfrom", int(a[1]))))
        elif rel == "accessor":
            deferred.append((int(a[0]), ("accessor", int(a[1]), s.atom(unesc(a[2])), int(a[3]))))
        elif rel == "protofrom":
            deferred.append((int(a[0]), ("protofrom", int(a[1]))))
        elif rel == "protoof":
            deferred.append((int(a[0]), ("protoof", int(a[1]))))
        elif rel == "iternext":
            deferred.append((int(a[0]), ("iternext", int(a[1]))))
        elif rel == "newtargethard":
            deferred.append((int(a[0]), ("newtargethard",)))
        elif rel == "arith":
            deferred.append((int(a[0]), ("arith", int(a[1]), a[2] == "1")))
        elif rel == "idxload":
            deferred.append((int(a[0]), ("idxload", int(a[1]), int(a[2]))))
        elif rel == "idxkey":
            deferred.append((int(a[0]), ("idxkey", int(a[1]), int(a[2]))))
        elif rel == "chainunion":
            deferred.append((int(a[0]), ("chainunion", key_of(s, a[1]), int(a[2]))))
        elif rel == "protoclosure":
            deferred.append((int(a[0]), ("protoclosure", int(a[1]))))
        elif rel == "anyaccessors":
            deferred.append((int(a[0]), ("anyaccessors", int(a[1]), parse_act(s, a[2]))))
        elif rel == "protomethods":
            pass
        elif rel == "storetopproto":
            deferred.append((int(a[0]), ("storetopproto", int(a[1]), int(a[2]))))
        elif rel == "sharebuffer":
            deferred.append((int(a[0]), ("sharebuffer", int(a[1]))))
        elif rel == "chain":
            deferred.append((int(a[0]), ("chain", int(a[1]), key_of(s, a[2]), parse_act(s, a[3]))))
        elif rel == "accinstall":
            deferred.append((int(a[0]), ("accinstall", s.atom(unesc(a[1])), int(a[2]), int(a[3]))))
        elif rel in ("escapeseed", "escaped"):
            deferred.append((None, ("escape", int(a[0]))))
        elif rel == "hardmark":
            deferred.append((None, ("hardseed", int(a[0]), a[1])))
        elif rel == "hardseed":
            deferred.append((None, ("hardseed", int(a[0]), a[1])))
        elif rel == "accseed":
            h, k, f, st = int(a[0]), int(a[1]), int(a[2]), int(a[3])
            lst = s.accessors[(h, k)][1 if st else 0]
            if f not in lst:
                lst.append(f)
            if k not in s.acc_keys[h]:
                s.acc_keys[h].append(k)
        elif rel == "whatif":
            s.whatif = True
        elif rel == "interp":
            s.interp = None if a[0] == "-" else unesc(" ".join(a))
        elif rel == "accstar":
            h = int(a[0])
            st = s.atom(STAR)
            if st not in s.acc_keys[h]:
                s.acc_keys[h].append(st)
        elif rel == "dyneval":
            s.dyn_eval = unesc(" ".join(a))
        elif rel == "readsite":
            s.read_sites.append((int(a[0]), int(a[1]), int(a[2]), int(a[3]), a[4] == "1"))
    for n, c in deferred:
        if n is None:
            if c[0] == "escape":
                s.escape(c[1])
            else:
                s.mark_hard(c[1], c[2])
        else:
            s.addcons(n, c)
    s.solve()


# ----------------------------------------------------------- derivation
def prim_name(bits):
    return "|".join(n for b, n in PRIM_NAMES if bits & b)


def chain_tokens(s, tm):
    t = s.tok_of.get(("obj", tm))
    if t is None:
        return []
    seen, out, stack = set(), [], [t]
    while stack:
        x = stack.pop()
        pn = s.protonode.get(x)
        if pn is None:
            continue
        for p in sorted(s.pts[pn]):
            if p not in seen:
                seen.add(p); out.append(p); stack.append(p)
    return out


def derive(s, objproto_hard):
    facts = {}
    tfacts = {}
    star = s.atom(STAR)
    op = s.tok_of.get(("objectproto", "-"))
    for tm, tp in s.tmpls.items():
        if tp["kind"] not in ("class", "lit", "fninst"):
            continue
        t = s.tok_of.get(("obj", tm))
        if t is None:
            t = s.tok("obj", tm)
        disp = tp["display"]
        escape = t in s.escaped or s.objproto_accessor
        hard = t in s.hard
        star_on_chain = star in s.acc_keys.get(t, [])
        chain_bad = False
        acc = list(tp["acc"]) + list(s.acc_keys.get(t, []))
        for p in chain_tokens(s, tm):
            if p == TOP:
                chain_bad = True; continue
            if p in s.hard:
                chain_bad = True
            if p != op and p in s.escaped:
                chain_bad = True
            acc += s.acc_keys.get(p, [])
            if star in s.acc_keys.get(p, []):
                star_on_chain = True
        if star_on_chain:
            escape = True
        e14 = (not s.whatif) and s.interp is not None and (s.dyn_eval is not None or s.top_calls > 0)
        if objproto_hard or e14 or s.objproto_accessor:
            chain_bad = True
        if star in acc or (tp["computed"] and not tp["birth"] and tp["kind"] != "fninst"):
            chain_bad = True
        if tp["native"] is not None and tp["native"] not in SUMMARIZED_NATIVE:
            chain_bad = True
        ps = not escape and not hard and not chain_bad
        adds = s.adds.get(tm, 0)
        fixed = ps and adds == 0
        keys = list(tp["birth"])
        for a in s.field_keys.get(t, []):
            if a not in keys:
                keys.append(a)
        slot_ok = True
        fields = {}
        for i, a in enumerate(keys):
            birth = i < len(tp["birth"])
            is_acc = a in acc
            definite = birth and a in tp["definite"] and not is_acc
            if birth and not definite:
                slot_ok = False; fixed = False
            slot = i if (birth and definite and slot_ok) else None
            w = s.writers.get((tm, a), [set(), set()])
            aw = s.any_writers.get(tm, [set(), set()])
            readonly = not escape and not hard and not w[1] and not aw[1] and not is_acc
            kind = "top" if escape else field_kind(s, t, a, definite)
            ta = readonly and definite and ta_sealed(s, t, a)
            fields[a] = dict(slot=slot, definite=definite, data=not is_acc, readonly=readonly,
                             kind=kind, ta=ta)
            name = esc(s.atoms[a])
            facts[f"F1 readonly {disp}.{name}"] = str(int(readonly))
            facts[f"F1' definite {disp}.{name}"] = str(int(definite))
            facts[f"F2 kind {disp}.{name}"] = kind
            facts[f"F6 ta_stable {disp}.{name}"] = str(int(ta))
        facts[f"F3 prefix_stable {disp}"] = str(int(ps))
        facts[f"F3 fixed_layout {disp}"] = str(int(fixed))
        facts[f"F3 escape {disp}"] = str(int(escape or hard))
        tfacts[tm] = dict(ps=ps, fixed=fixed, fields=fields, disp=disp)
    return facts, tfacts


def field_kind(s, t, a, definite):
    prims = 0 if definite else UNDEF
    top = t in s.escaped
    objs, funcs, bobjs = set(), 0, 0
    n = s.fields.get((t, a))
    if n is not None:
        prims |= s.prim[n]
        for x in s.pts[n]:
            k = s.toks[x]
            if k[0] == "top":
                top = True
            elif k[0] == "obj" and s.tmpls[k[1]]["kind"] in ("class", "lit", "fninst"):
                objs.add(s.tmpls[k[1]]["display"])
            elif k[0] in ("func", "bound", "builtinfn"):
                funcs += 1
            else:
                bobjs += 1
    if top:
        return "top"
    parts = []
    if prims:
        parts.append(prim_name(prims))
    if objs:
        parts.append("obj{" + ",".join(sorted(objs)) + "}")
    if funcs:
        parts.append(f"fn*{funcs}")
    if bobjs:
        parts.append(f"builtin*{bobjs}")
    return "|".join(parts) if parts else "none"


def ta_sealed(s, t, a):
    n = s.fields.get((t, a))
    if n is None or (s.prim[n] & ~UNDEF):
        return False
    toks = s.pts[n]
    if not toks:
        return False
    for x in toks:
        k = s.toks[x]
        if not (k[0] == "obj" and s.tmpls[k[1]]["kind"] == "bobj" and s.tmpls[k[1]]["what"] == "TypedArray"):
            return False
        if x in s.hard or x in s.escaped:
            return False
    return True


def site_verdicts(s, tfacts):
    out = {}
    for (m, off, key, recv, is_call) in s.read_sites:
        toks = sorted(s.pts[recv])
        verdict = None
        slots = []
        targets = set()
        recvs = 0
        for t in toks:
            k = s.toks[t]
            recvs += 1
            if k[0] == "top":
                verdict = verdict or "no"; continue
            if k[0] != "obj" or k[1] not in tfacts:
                verdict = verdict or "no"; continue
            tf = tfacts[k[1]]
            if not tf["ps"]:
                verdict = verdict or "no"; continue
            if is_call:
                tg = resolve_method(s, t, key)
                if tg is None:
                    verdict = verdict or "no"
                else:
                    targets.add(tg)
                continue
            f = tf["fields"].get(key)
            if f and f["definite"] and f["data"] and f["slot"] is not None:
                slots.append(f["slot"])
            else:
                verdict = verdict or "no"
        if not toks:
            verdict = verdict or "no"
        if verdict is None:
            if is_call:
                verdict = ("T0" if recvs == 1 else "T0set") if len(targets) == 1 else "no"
            else:
                verdict = ("T0" if recvs == 1 else "T0set") if len(set(slots)) <= 1 else "no"
        out[(m, off, s.atoms[key], int(is_call))] = merge_verdict(out.get((m, off, s.atoms[key], int(is_call))), verdict)
    return out


def merge_verdict(a, b):
    if a is None:
        return b
    return a if a == b else "mixed"


def resolve_method(s, t, key):
    cur = [t]
    seen = set()
    while cur:
        found = []
        for h in cur:
            n = s.fields.get((h, key))
            if n is not None:
                for x in s.pts[n]:
                    if x not in found:
                        found.append(x)
            fa = s.fieldany.get(h)
            if fa is not None and s.pts[fa]:
                return None
        if found:
            if len(found) == 1 and s.toks[found[0]][0] == "func":
                return found[0]
            return None
        nxt = []
        for h in cur:
            pn = s.protonode.get(h)
            if pn is not None:
                for p in sorted(s.pts[pn]):
                    if p not in seen:
                        seen.add(p); nxt.append(p)
        cur = nxt
    return None


STRONGER = {"1": 1, "0": 0}


def compare(rust, py, rust_sites, py_sites):
    sound, prec, cover = [], [], []
    for k, rv in rust.items():
        pv = py.get(k)
        if pv is None:
            # A key only the compiler lists: harmless when the compiler's
            # value is the weakest one (nothing is claimed about it).
            fact = k.split(" ")[1]
            weakest = "top" if fact == "kind" else ("1" if fact == "escape" else "0")
            if rv != weakest:
                sound.append(f"{k}: compiler={rv} recompute=<absent>")
            else:
                cover.append(k)
            continue
        if rv == pv:
            continue
        fact = k.split(" ")[1]
        if fact == "kind":
            # Narrower kind claimed by the compiler = soundness problem.
            if rv == "top" or (pv != "top" and set(pv.split("|")) <= set(rv.split("|"))):
                prec.append(f"{k}: compiler={rv} recompute={pv}")
            else:
                sound.append(f"{k}: compiler={rv} recompute={pv}")
        elif fact == "escape":
            (sound if rv == "0" else prec).append(f"{k}: compiler={rv} recompute={pv}")
        else:
            (sound if rv == "1" else prec).append(f"{k}: compiler={rv} recompute={pv}")
    for k in py:
        if k not in rust:
            prec.append(f"{k}: compiler=<absent> recompute={py[k]}")
    for k, rv in rust_sites.items():
        pv = py_sites.get(k)
        if pv is None or rv == pv:
            continue
        if rv in ("T0", "T0set") and pv not in ("T0", "T0set"):
            sound.append(f"site {k}: compiler={rv} recompute={pv}")
        else:
            prec.append(f"site {k}: compiler={rv} recompute={pv}")
    return sound, prec, cover


def read_rust(dump):
    facts, sites, objproto_hard = {}, {}, False
    for line in open(dump):
        line = line.rstrip("\n")
        if line.startswith("FACT "):
            body, val = line[5:].rsplit(" = ", 1)
            facts[body] = val
        elif line.startswith("S "):
            p = line.split(" ")
            m, off, key, call = int(p[1]), int(p[2]), p[3][1:], int(p[4].split("=")[1])
            v = line.rsplit("verdict=", 1)[1]
            v = "no" if v.startswith("no:") else v
            sites[(m, off, key, call)] = merge_verdict(sites.get((m, off, key, call)), v)
        elif line.startswith("T ") and "Object.prototype" in line:
            objproto_hard = True
    return facts, sites, objproto_hard


def main():
    dump = sys.argv[1]
    quiet = "--quiet" in sys.argv
    rust, rust_sites, _ = read_rust(dump)
    s, rel = load(dump + ".rel")
    run(s, rel)
    op = s.tok_of.get(("objectproto", "-"))
    objproto_hard = op in s.hard if op is not None else False
    py, tfacts = derive(s, objproto_hard)
    py_sites = site_verdicts(s, tfacts)
    sound, prec, cover = compare(rust, py, rust_sites, py_sites)
    print(f"recompute: {len(rust)} compiler facts, {len(py)} recomputed, "
          f"{len(rust_sites)} sites; soundness diffs {len(sound)}, precision diffs {len(prec)}, "
          f"weakest-only keys {len(cover)}")
    if not quiet:
        for d in sound[:50]:
            print("  SOUNDNESS", d)
        for d in prec[:50]:
            print("  PRECISION", d)
    sys.exit(0 if not sound and not prec else 1)


if __name__ == "__main__":
    main()
