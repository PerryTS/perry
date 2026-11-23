#!/usr/bin/env python3
"""Behavioural check of the realm's effect declarations.

A built-in's runtime definition may declare what it does to its inputs
(`#[realm(Path, effects = { ... })]`, grammar in
crates/perry-runtime-manifest/src/realm_effects.rs). The call-graph check
(`perry-gc-effects effects`) proves only that a native which can reach
program code declares some way to do so. This script checks the other
direction: it runs every declared native on instrumented inputs, compiled by
perry, and requires that everything the program OBSERVES is covered by what
the declaration says (observed is a subset of declared).

Everything is generated from the declarations in the realm manifest
(`realm-v1.json`, the source of the perry-realm-table static): the natives,
how each is called (which argument is a callback, which a value, whether
the receiver matters) and what each observed event needs. No native is named
here.

Per declared native and receiver scenario one TypeScript program is written,
compiled and run. It prints `EV ...` lines:

    EV trap <id> <op> <key> [<value id>]   a Proxy trap on a probe
    EV coerce <id> <hint>                  Symbol.toPrimitive ran on a probe
    EV call <id> this=<id> args=<id,...>   a callback probe was called
    EV construct <id> args=<id,...>        a species constructor probe ran
    EV mutate <id> <key> <value id>        a real-array receiver changed
    EV result <id> / EV result-elem <i> <id>
    EV alive <id> in=<this|result|none>    a probe kept alive after gc()
    EV completion throw                 caught exception (no normal return)
    EV end

A probe id is `<slot>#<n>` (the slot it was passed as, in the grammar's
words: `this`, `arg(1)`, `elems(this)`, `returned(0)`, ...), `prim:<kind>`
for primitives, `obj:<prototype>` for objects that are no probe. The
comparator maps each event to the effects that would cover it and reports
the first uncovered event per native and scenario, naming what it needs.

Usage:
    behave.py --perry BIN --realm realm-v1.json --out DIR
              [--only NAME] [--weaken NAME=verb ...] [--node NODE]
Exit 1 when any declaration is red.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

# Ordinary element objects have no Symbol.toPrimitive hook, exposing toString/valueOf.
SCENARIOS = ("plain", "species", "proxy", "ordinary")

# ---------------------------------------------------------------- slots


def slot_text(slot: dict) -> str:
    """A manifest slot as the grammar writes it: `elems(this)`, `arg(0)`."""
    root = slot["root"]
    if isinstance(root, str):
        text = root
    else:
        ((kind, n),) = root.items()
        text = f"{kind}({n})"
    for step in slot.get("steps", []):
        if step == "elems":
            text = f"elems({text})"
        else:
            ((kind, key),) = step.items()
            text = f"{kind}({text},{key})"
    return text


def operand_text(op: dict) -> str:
    ((kind, v),) = op.items()
    if kind == "slot":
        return slot_text(v)
    if kind == "prim":
        return "prim(" + " ".join(v) + ")"
    if kind == "union":
        return " | ".join(operand_text(o) for o in v)
    if kind == "fresh":
        elems = ", ".join(operand_text(e) for e in v.get("elems", []))
        fields = ", ".join(f"{f['key']}: {operand_text(f['value'])}" for f in v.get("fields", []))
        return f"fresh({v['proto']}, elems = [{elems}], fields = {{{fields}}})"
    if kind == "native":
        return f"native({v})"
    if kind == "species":
        args = ", ".join(operand_text(o) for o in v.get("args", []))
        elems = ", ".join(slot_text(s) for s in v.get("elems", []))
        return f"species({slot_text(v['of'])}, args = [{args}], elems = [{elems}])"
    raise ValueError(f"unknown operand {op!r}")


def verb(effect) -> str:
    return effect if isinstance(effect, str) else next(iter(effect))


def all_slots(effects) -> list[str]:
    """Every slot a declaration names, as text."""
    out: list[str] = []

    def op(o):
        ((kind, v),) = o.items()
        if kind == "slot":
            out.append(slot_text(v))
        elif kind == "union":
            for m in v:
                op(m)
        elif kind == "fresh":
            for e in v.get("elems", []):
                op(e)
            for f in v.get("fields", []):
                op(f["value"])
        elif kind == "species":
            out.append(slot_text(v["of"]))
            for a in v.get("args", []):
                op(a)
            out.extend(slot_text(s) for s in v.get("elems", []))

    for e in effects:
        if isinstance(e, str):
            continue
        ((kind, v),) = e.items()
        if kind == "reads":
            out.extend(slot_text(s) for s in v)
        elif kind == "coerce":
            out.append(slot_text(v["slot"]))
        elif kind == "calls":
            out.append(slot_text(v["callee"]))
            op(v["this"])
            for a in v["args"]:
                op(a)
        elif kind == "stores":
            out.append(slot_text(v["src"]))
            out.append(slot_text(v["dst"]))
        elif kind == "returns":
            op(v)
    return out


def slot_of(ident: str) -> str:
    return ident.split("#", 1)[0]


def root_of(slot: str) -> str:
    """The receiver/argument a slot hangs from: `elems(this)` -> `this`."""
    while True:
        if slot.startswith("elems(") and slot.endswith(")"):
            slot = slot[len("elems("):-1]
        elif slot.startswith("field(") and slot.endswith(")"):
            slot = slot[len("field("):-1].rsplit(",", 1)[0]
        else:
            return slot


# ------------------------------------------------------------ the shape


def call_shape(effects, length: int = 0, method: bool = False) -> dict:
    """How to call the native, read from its realm declaration: the slots its
    effects name, its `length` and whether it is a prototype method. One
    argument more than the declaration and `length` name is passed (a rest
    list gets two probes from its index on), so a body that touches an argument its declaration
    never mentions is observed too."""
    callees: dict[int, int] = {}  # arg index -> index of its `calls`
    calls = [e["calls"] for e in effects if verb(e) == "calls"]
    for i, c in enumerate(calls):
        callee = slot_text(c["callee"])
        if callee.startswith("arg("):
            callees.setdefault(int(callee[4:-1]), i)
    slots = all_slots(effects)
    args = [int(s[4:-1]) for s in map(root_of, slots) if s.startswith("arg(")]
    rests = [int(s[5:-1]) for s in map(root_of, slots) if s.startswith("rest(")]
    if rests:
        # Arguments from the rest index on are the rest list (two probes).
        nargs = min(rests) + 2
    else:
        nargs = max(args + [length - 1]) + 1 + 1
    return {
        "receiver": method or any(root_of(s) == "this" for s in slots),
        "nargs": nargs,
        "callees": callees,
        "rest": min(rests) if rests else None,
        "protos": sorted(
            {o["fresh"]["proto"] for e in effects if verb(e) == "returns"
             for o in _members(e["returns"]) if "fresh" in o} | {"Array.prototype", "Object.prototype"}
        ),
    }


def _members(op):
    if "union" in op:
        for m in op["union"]:
            yield from _members(m)
    else:
        yield op


# ------------------------------------------------------------ generator

PRELUDE = r"""
"use strict";
declare function gc(): void;
const ids = new WeakMap<object, string>();
let on = true;
function out(s: string): void { if (on) console.log("EV " + s); }
function protoName(v: any): string {
  const p = Object.getPrototypeOf(v);
  if (p === null) return "null";
__PROTOS__
  return "other";
}
function idOf(v: any): string {
  if (v === null) return "prim:null";
  if (v === globalThis) return "obj:globalThis";
  const t = typeof v;
  if (t === "number") return "prim:num";
  if (t === "string") return "prim:str";
  if (t === "boolean") return "prim:bool";
  if (t === "undefined") return "prim:undefined";
  if (t === "bigint") return "prim:bigint";
  if (t === "symbol") return "prim:symbol";
  const s = ids.get(v);
  if (s !== undefined) return s;
  return "obj:" + protoName(v);
}
function key(k: any): string {
  if (typeof k === "symbol") {
    const d = String(k.description);
    return "@@" + (d.startsWith("Symbol.") ? d.slice(7) : d);
  }
  return String(k);
}
function handler(id: string): any {
  return {
    get(t: any, k: any, r: any) { out("trap " + id + " get " + key(k)); return Reflect.get(t, k, r); },
    has(t: any, k: any) { out("trap " + id + " has " + key(k)); return Reflect.has(t, k); },
    set(t: any, k: any, v: any, r: any) { out("trap " + id + " set " + key(k) + " " + idOf(v)); return Reflect.set(t, k, v); },
    defineProperty(t: any, k: any, d: any) {
      out("trap " + id + " defineProperty " + key(k) + " " + ("value" in d ? idOf(d.value) : "accessor"));
      return Reflect.defineProperty(t, k, d);
    },
    deleteProperty(t: any, k: any) { out("trap " + id + " deleteProperty " + key(k)); return Reflect.deleteProperty(t, k); },
    ownKeys(t: any) { out("trap " + id + " ownKeys -"); return Reflect.ownKeys(t); },
    getOwnPropertyDescriptor(t: any, k: any) { out("trap " + id + " getOwnPropertyDescriptor " + key(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
    getPrototypeOf(t: any) { out("trap " + id + " getPrototypeOf -"); return Reflect.getPrototypeOf(t); },
  };
}
function probe(id: string, target: any): any {
  const p = new Proxy(target, handler(id));
  ids.set(p, id);
  ids.set(target, id);
  return p;
}
// A value probe: every trap is logged, ToPrimitive is logged and gives 0.
function value(id: string, ordinary: boolean = false): any {
  const t: any = {};
  if (!ordinary) t[Symbol.toPrimitive] = function (hint: string) { out("coerce " + id + " " + hint); return 0; };
  t.toString = function () { out("coerce " + id + " toString"); return "0"; };
  t.valueOf = function () { out("coerce " + id + " valueOf"); return 0; };
  return probe(id, t);
}
function list(xs: any[]): string { let s = ""; for (let i = 0; i < xs.length; i++) s += (i ? "," : "") + idOf(xs[i]); return s; }
function contents(o: any): string[] {
  const r: string[] = [];
  for (let i = 0; i < o.length; i++) r.push(idOf(o[i]));
  return r;
}
"""

SCENARIO = r"""
// ---- receiver (__SCENARIO__)
const ordinary = "__SCENARIO__" === "ordinary";
const elems: any[] = [value("elems(this)#0", ordinary), value("elems(this)#1", ordinary), value("elems(this)#2", ordinary)];
const target: any = elems.slice();
let recv: any = target;
ids.set(target, "this");
__RECEIVER__
if ("__SCENARIO__" === "proxy") recv = probe("this", target);
if ("__SCENARIO__" === "species") {
  const ctorT: any = {};
  const ctor = probe("field(this,constructor)", ctorT);
  const speciesCtor: any = function (this: any, ...args: any[]) {
    out("construct field(field(this,constructor),@@species) args=" + list(args));
    return probe("species(this)#0", []);
  };
  ids.set(speciesCtor, "field(field(this,constructor),@@species)");
  ctorT[Symbol.species] = speciesCtor;
  Object.defineProperty(target, "constructor", {
    configurable: true,
    get() { out("trap this get constructor"); return ctor; },
  });
}
let F: any = __ROOT__;
for (const k of __PATH__) F = F[k];
const before = contents(target);
const weak: any[] = [];
let nret = 0;
function hold(x: any): any { weak.push(new WeakRef(x)); return x; }
function callback(id: string, k: number): any {
  const f: any = function (this: any, ...args: any[]) {
    out("call " + id + " this=" + idOf(this) + " args=" + list(args));
    return hold(value("returned(" + k + ")#" + (nret++)));
  };
  ids.set(f, id);
  return f;
}
function run(): any {
  const args: any[] = [];
__ARGS__
  __RETURN__;
}
// Abrupt completion has no normal return. Effects already observed still
// count; catching a missing mandatory callback keeps those probes observable.
let normal = false;
let result: any;
try { result = run(); normal = true; }
catch (error) { out("completion throw"); }
on = false;
const after = contents(target);
on = true;
for (let i = 0; i < Math.max(before.length, after.length); i++)
  if (before[i] !== after[i]) out("mutate this " + i + " " + (after[i] === undefined ? "prim:undefined" : after[i]));
if (before.length !== after.length) out("mutate this length prim:num");
if (normal) out("result " + idOf(result));
on = false;
const rid = normal ? idOf(result) : "";
const rel: string[] = (rid.startsWith("obj:") || rid.startsWith("species(")) && result.length !== undefined ? contents(result) : [];
on = true;
for (let i = 0; i < rel.length; i++) out("result-elem " + i + " " + rel[i]);
on = false;
(globalThis as any).__probe_result = result;
result = undefined;
gc();
gc();
const inThis = contents(target);
const inResult = rel;
const alive: string[] = [];
for (const w of weak) { const v = w.deref(); if (v !== undefined) alive.push(idOf(v)); }
on = true;
for (const a of alive)
  out("alive " + a + " in=" + (inThis.indexOf(a) >= 0 ? "this" : inResult.indexOf(a) >= 0 ? "result" : "none"));
if ((globalThis as any).__probe_result === 1) out("unreachable");
out("end");
"""


def generate(decl: dict, scenario: str, form: str = "apply", npass: int | None = None,
             receiver: str | None = None) -> str:
    """The probe program for one declaration, receiver scenario and call form:
    `apply` calls the native value reflectively (`F.apply(recv, args)`),
    `method` as a program writes it with argument expressions
    (`recv.name(args[0])`), `local` with argument variables
    (`recv.name(a0)`): the compiler and the runtime's method dispatch route
    these to different bodies. `receiver`, a TypeScript expression, is
    the receiver instead of the scenario's array (`--receiver`). `npass`
    positional arguments are passed (default all), including up to two
    probes from a declared rest index."""
    effects = decl["effects"]
    shape = shape_of(decl)
    protos = "\n".join(
        f'  if (p === {p}) return "{p}";' for p in shape["protos"]
    )
    path = decl["name"].split(".")
    # A node module member (`node:fs.existsSync`) hangs from the module value.
    root = "globalThis"
    if path[0].startswith("node:"):
        root = f'require({json.dumps(path[0][len("node:"):])})'
        path = path[1:]
    lines = []
    npass = shape["nargs"] if npass is None else npass
    for i in range(npass):
        if shape["rest"] is not None and i >= shape["rest"]:
            lines.append(f'  args.push(hold(value("rest({shape["rest"]})#{i - shape["rest"]}")));')
        elif i in shape["callees"]:
            lines.append(f'  args.push(hold(callback("arg({i})", {shape["callees"][i]})));')
        else:
            lines.append(f'  args.push(hold(value("arg({i})#0")));')
    recv = "recv" if shape["receiver"] else "undefined"
    n = npass
    if form == "method":
        call = f"recv.{path[-1]}(" + ", ".join(f"args[{i}]" for i in range(n)) + ")"
    elif form == "local":
        binds = "".join(f"const a{i} = args[{i}]; " for i in range(n))
        call = binds + f"return recv.{path[-1]}(" + ", ".join(f"a{i}" for i in range(n)) + ")"
    else:
        call = f"F.apply({recv}, args)"
    return (
        PRELUDE.replace("__PROTOS__", protos)
        + SCENARIO.replace("__SCENARIO__", scenario)
        .replace("__RECEIVER__", f"recv = ({receiver}); ids.set(recv, \"this\");" if receiver else "")
        .replace("__ROOT__", root)
        .replace("__PATH__", json.dumps(path))
        .replace("__ARGS__", "\n".join(lines + [f'  out("args {npass}");']))
        .replace("__RETURN__", call if form == "local" else f"return {call}")
    )


def is_method(decl: dict) -> bool:
    return decl["name"].split(".")[-2:-1] == ["prototype"]


def shape_of(decl: dict) -> dict:
    return call_shape(decl["effects"], int(decl.get("length", 0)), is_method(decl))


def scenarios(decl: dict, receiver: str | None = None) -> list[tuple[str, str, int]]:
    """(receiver scenario, call form, positional arguments passed): the
    receiver kinds only when the declaration speaks about `this`, the
    method-call form only for a prototype method, and every argument count
    from zero up to every argument passed (an omitted optional argument
    can select another body; a missing mandatory callback may throw). A
    given `receiver` is the one receiver kind."""
    shape = shape_of(decl)
    counts = range(shape["nargs"] + 1)
    if receiver:
        forms = ("apply", "method", "local") if is_method(decl) else ("apply",)
        return [("plain", f, n) for f in forms for n in counts]
    if not shape["receiver"]:
        return [("plain", "apply", n) for n in counts]
    forms = ("apply", "method", "local") if is_method(decl) else ("apply",)
    return [(sc, f, n) for sc in SCENARIOS for f in forms for n in counts]


# ----------------------------------------------------------- comparator


def covers(op: dict, ident: str, absent: frozenset = frozenset()) -> bool:
    """Does the operand cover the observed value `ident`? `absent`: the
    argument slots the call did not pass (their value is undefined)."""
    ((kind, v),) = op.items()
    if kind == "slot":
        return slot_of(ident) == slot_text(v) or (ident == "prim:undefined" and slot_text(v) in absent)
    if kind == "prim":
        return ident in {f"prim:{k}" for k in v}
    if kind == "union":
        return any(covers(m, ident, absent) for m in v)
    if kind == "fresh":
        return ident == f"obj:{v['proto']}"
    if kind == "species":
        # ArraySpeciesCreate: the program constructor's value, else a fresh Array.
        return slot_of(ident) == f"species({slot_text(v['of'])})" or ident == "obj:Array.prototype"
    return False


def _returns(effects):
    for e in effects:
        if verb(e) == "returns":
            yield from _members(e["returns"])


def _species(effects, of: str):
    for o in _returns(effects):
        if "species" in o and slot_text(o["species"]["of"]) == of:
            yield o["species"]


def event_error(event: list[str]) -> str | None:
    """Reject events this comparator cannot interpret, including new trap kinds."""
    if not event:
        return "open (empty event)"
    kind = event[0]
    n = len(event)
    if kind == "trap":
        valid = (n == 5 and event[2] in ("set", "defineProperty")) or (
            n == 4 and event[2] in ("get", "has", "deleteProperty", "ownKeys",
                                   "getOwnPropertyDescriptor", "getPrototypeOf"))
    elif kind == "call":
        valid = n == 4 and event[2].startswith("this=") and event[3].startswith("args=")
    elif kind == "construct":
        valid = (n == 3 and event[1].startswith("field(field(")
                 and event[1].endswith(",constructor),@@species)") and event[2].startswith("args="))
    elif kind == "coerce":
        valid = n == 3 and event[2] in ("default", "number", "string", "toString", "valueOf")
    elif kind == "mutate":
        valid = n == 4
    elif kind == "result":
        valid = n == 2
    elif kind == "result-elem":
        valid = n == 3 and event[1].isdigit()
    elif kind == "alive":
        valid = n == 3 and event[2] in ("in=this", "in=result", "in=none")
    elif kind == "args":
        valid = n == 2 and event[1].isdigit()
    elif kind == "completion":
        valid = event == ["completion", "throw"]
    elif kind == "end":
        valid = n == 1
    else:
        valid = False
    return None if valid else f"open (unknown or malformed event {' '.join(event)})"


def needs(event: list[str], effects, events) -> str | None:
    """None when the declaration covers the event, else the effect it needs."""
    error = event_error(event)
    if error is not None:
        return error
    if any(verb(e) == "open" for e in effects):
        return None
    kind = event[0]
    npass = next((int(ev[1]) for ev in events if ev and ev[0] == "args" and event_error(ev) is None), 0)
    absent = frozenset(s for s in map(root_of, all_slots(effects))
                       if s.startswith("arg(") and int(s[4:-1]) >= npass)
    reads = {slot_text(s) for e in effects if verb(e) == "reads" for s in e["reads"]}
    coerces = {(slot_text(e["coerce"]["slot"]), e["coerce"]["protocol"]) for e in effects if verb(e) == "coerce"}
    stores = [(slot_text(e["stores"]["src"]), slot_text(e["stores"]["dst"])) for e in effects if verb(e) == "stores"]

    def store(dst_root: str, val: str | None, key: str) -> str | None:
        for src, dst in stores:
            if root_of(dst) != dst_root:
                continue
            if val is None or slot_of(val) == src:
                return None
            # The length of an array is part of its elements.
            if key == "length" and val == "prim:num" and dst == f"elems({dst_root})":
                return None
        src = slot_of(val) if val else "?"
        return f"stores({src} -> elems({dst_root}))"

    if kind == "trap":
        ident, op, key = event[1], event[2], event[3]
        val = event[4] if len(event) > 4 and event[4] != "accessor" else None
        slot = slot_of(ident)
        if slot.startswith("species("):
            of = slot[len("species("):-1]
            for sp in _species(effects, of):
                if val is None or any(slot_of(val) == slot_text(s) for s in sp.get("elems", [])):
                    return None
            return f"returns(species({of}, elems = [{slot_of(val) if val else ''}]))"
        if op in ("get", "has", "ownKeys", "getOwnPropertyDescriptor", "getPrototypeOf"):
            # `reads(s)`: any key of s; `reads(field(s, k))`: the key k of s.
            if slot in reads or (op in ("get", "has") and f"field({slot},{key})" in reads):
                return None
            if key in ("@@toPrimitive", "valueOf", "toString") and (slot, "to_primitive") in coerces:
                return None
            if key in ("@@iterator", "next") and ((slot, "iterate") in coerces or (slot, "iter_next") in coerces):
                return None
            if slot.startswith("field(") and slot.endswith(",constructor)") and key == "@@species":
                of = slot[len("field("):-len(",constructor)")]
                if any(True for _ in _species(effects, of)):
                    return None
            if key in ("@@toPrimitive", "valueOf", "toString"):
                return f"coerce({slot}, ToPrimitive)"
            return f"reads({slot})"
        return store(slot, val, key)
    if kind == "coerce":
        slot = slot_of(event[1])
        return None if (slot, "to_primitive") in coerces else f"coerce({slot}, ToPrimitive)"
    if kind == "mutate":
        return store(event[1], event[3], event[2])
    if kind == "call":
        callee = slot_of(event[1])
        this = event[2][len("this="):]
        args = [a for a in event[3][len("args="):].split(",") if a]
        for e in effects:
            if verb(e) != "calls" or slot_text(e["calls"]["callee"]) != callee:
                continue
            c = e["calls"]
            if covers(c["this"], this, absent) and len(args) <= len(c["args"]) and all(
                covers(o, a, absent) for o, a in zip(c["args"], args)
            ):
                return None
        return f"calls({callee}, this = {slot_of(this)}, args = [{', '.join(map(slot_of, args))}])"
    if kind == "construct":
        of = event[1][len("field(field("):-len(",constructor),@@species)")]
        args = [a for a in event[2][len("args="):].split(",") if a]
        for sp in _species(effects, of):
            ops = sp.get("args", [])
            if len(args) <= len(ops) and all(covers(o, a) for o, a in zip(ops, args)):
                return None
        return f"returns(species({of}, args = [{', '.join(map(slot_of, args))}]))"
    if kind == "result":
        ident = event[1]
        return None if any(covers(o, ident) for o in _returns(effects)) else f"returns({slot_of(ident)})"
    if kind == "result-elem":
        rid = next((ev[1] for ev in events if ev[0] == "result"), "")
        el = slot_of(event[2])
        for o in _returns(effects):
            if not covers(o, rid):
                continue
            ((k, v),) = o.items()
            if k == "fresh" and any(covers(e, event[2], absent) for e in v.get("elems", [])):
                return None
            if k == "species" and el in {slot_text(s) for s in v.get("elems", [])}:
                return None
        return f"returns(.. elems = [{el}])"
    if kind == "alive":
        ident, where = event[1], event[2][len("in="):]
        slot = slot_of(ident)
        if where == "this":
            return store("this", ident, "")
        if where == "result":
            return None  # result-elem checked that the result may hold it
        return f"open ({slot} retained where the program cannot see it)"
    if kind in ("end", "args", "completion"):
        # Control metadata. Effects before a throw still require coverage;
        # returns describes normal completion only.
        return None
    return f"open (unknown event {' '.join(event)})"


def parse(text: str) -> list[list[str]]:
    return [line[3:].split(" ") for line in text.splitlines() if line.startswith("EV ")]


def check(decl: dict, text: str) -> tuple[list[str], int]:
    """(reds, number of events) for one program's output."""
    events = parse(text)
    reds = []
    for ev in events:
        need = needs(ev, decl["effects"], events)
        if need is not None:
            reds.append(f"{' '.join(ev)} needs {need}")
    if not events or events[-1] != ["end"]:
        reds.append("program did not finish (no `end` event)")
    return reds, len(events)


def weaken(effects, verb_name: str):
    return [e for e in effects if verb(e) != verb_name]


# ---------------------------------------------------------------- driver


def declarations(realm: Path) -> list[dict]:
    data = json.loads(realm.read_text())
    return [d for d in data["declarations"] if d.get("effects")]


def compile_run(perry: str, src: Path, env) -> tuple[str, str]:
    exe = src.with_suffix("")
    c = subprocess.run([perry, "compile", str(src), "-o", str(exe)], capture_output=True, text=True, env=env, timeout=600)
    if c.returncode != 0:
        return "", f"compile rc={c.returncode}: {(c.stdout + c.stderr).strip().splitlines()[-1:]}"
    r = subprocess.run([str(exe)], capture_output=True, text=True, env=env, timeout=60, stdin=subprocess.DEVNULL)
    exe.unlink(missing_ok=True)
    return r.stdout, "" if r.returncode == 0 else f"run rc={r.returncode}"


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--perry", required=True)
    ap.add_argument("--realm", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--only", action="append", default=[])
    ap.add_argument("--weaken", action="append", default=[], help="NAME=verb: drop that verb (sabotage)")
    ap.add_argument("--receiver", help="TypeScript expression: the receiver of every probe "
                    "(a prototype method of a host kind, `Buffer.from(\"ab\")`); default the scenario arrays")
    ap.add_argument("--node", help="also run each program under node and report differing event logs (info)")
    a = ap.parse_args(argv)
    weak = dict(w.split("=", 1) for w in a.weaken)
    decls = declarations(a.realm)
    if a.only:
        decls = [d for d in decls if d["name"] in a.only]
    if not decls:
        print("RED no declaration with effects found")
        return 1
    a.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_NO_CACHE="1")
    red = 0
    for d in decls:
        # Sabotage only the claimed effects. Keep the probes and call forms
        # identical so removing calls cannot remove the callback being tested.
        claim = dict(d, effects=weaken(d["effects"], weak[d["name"]])) if d["name"] in weak else d
        for scenario, form, npass in scenarios(d, a.receiver):
            base = a.out / f"{d['name']}.{scenario}.{form}{npass}"
            sc = f"{scenario}/{form}/{npass}"
            src = Path(str(base) + ".ts")
            src.write_text(generate(d, scenario, form, npass, a.receiver))
            text, err = compile_run(a.perry, src, env)
            Path(str(base) + ".log").write_text(text)
            reds, n = check(claim, text)
            if err:
                reds.insert(0, err)
            if reds:
                red += 1
                for r in reds:
                    print(f"RED {d['name']} {sc}: {r}")
            else:
                print(f"GREEN {d['name']} {sc} ({n} events)")
            if a.node:
                mts = Path(str(base) + ".mts")
                mts.write_text(src.read_text())
                nr = subprocess.run([a.node, "--expose-gc", str(mts)], capture_output=True, text=True, timeout=60)
                theirs = [l for l in nr.stdout.splitlines() if l.startswith("EV ")]
                ours = [l for l in text.splitlines() if l.startswith("EV ")]
                if theirs != ours:
                    Path(str(base) + ".node.log").write_text(nr.stdout)
                    only_n = [l for l in theirs if l not in ours][:3]
                    only_p = [l for l in ours if l not in theirs][:3]
                    print(f"PARITY {d['name']} {sc}: node-only {only_n} perry-only {only_p}")
    print(f"natives={len(decls)} red_programs={red}")
    return 1 if red else 0


if __name__ == "__main__":
    sys.exit(main())
