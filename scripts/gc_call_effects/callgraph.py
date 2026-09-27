#!/usr/bin/env python3
"""Runtime call-graph classifier: the source of truth for codegen's GC call effects.

WHAT THIS ANSWERS

For every exported C symbol of the linked runtime archives (`libperry_runtime`,
`libperry_stdlib`), which of four classes it is in (RFC "deferred collection",
docs/src/internals/rfc-deferred-collection.md):

  Leaf       reaches no seed of any kind: it can neither start a collection nor
             re-enter JavaScript. The ONLY class codegen marks
             `"gc-leaf-function"` under today's runtime (the RFC's L2 set).
  AllocOnly  reaches a seed only through a collector entry point (the
             allocation trigger, a direct collection). Becomes a leaf once the
             runtime stops collecting at allocation points (RFC step S3/S4).
  ThrowOnly  additionally reaches a seed only through the throw funnel. Becomes
             a leaf with the throw cut (RFC step S5).
  Reenters   everything else: can reach JavaScript, a poll, an indirect call
             that was not audited, or a symbol outside the archives.

HOW

A symbol-level graph is built from `llvm-objdump -d -r` over the archives:

  * nodes are function symbols (aliases at one address are one node);
  * edges are DIRECT branches only: a call/jump instruction carrying a
    relocation. GOT/IAT calls (`call *foo@GOTPCREL`, `__imp_foo`, the arm64
    `adrp/ldr/blr` GOT sequence) are direct. An address that is merely taken
    (`lea`, data relocations, vtables) is not an edge: the indirect call that
    eventually uses it is itself a seed, so no edge is needed for soundness;
  * an indirect call or indirect tail jump is a SEED ("indirect") unless the
    containing function matches an audited exemption (seeds.txt, `indirect`
    rules, each with a reason); a jump through a compiler-emitted switch table
    is not an indirect call and is recognized structurally (see
    `_is_switch_dispatch`);
  * a direct call to a symbol no archive defines is a SEED ("extern") unless
    the symbol is on the system-library allowlist (seeds.txt, `extern`);
  * named seeds (collector entry points, polls, JS-invoking entry points) come
    from seeds.txt. Every named seed pattern must match at least one node: a
    pattern that matches nothing (renamed, inlined away) fails the run instead
    of silently seeding nothing.

The classes are three reverse-reachability fixed points over the same graph
with different cut sets (a cut node is a sink: nothing reached only through it
counts). See `classify`.

Deterministic: every output is sorted, and nothing depends on hash order.

USAGE

  callgraph.py generate --target T --out FILE ARCHIVE...
  callgraph.py check    --target T --table FILE ARCHIVE...
  callgraph.py why      SYMBOL... -- ARCHIVE...     (see why.py)
  callgraph.py --self-test

The committed tables live in crates/perry-codegen/src/gc_effects/<target>.tsv
and are read by `gc_call_effects::classify_direct_callee`.
"""
from __future__ import annotations

import argparse
import collections
import concurrent.futures
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
TABLE_DIR = os.path.join(ROOT, "crates", "perry-codegen", "src", "gc_effects")
TARGETS = ("linux-x86_64", "macos-aarch64", "windows-x86_64")
CLASSES = ("Leaf", "AllocOnly", "ThrowOnly", "Reenters")
CLASS_RANK = {c: i for i, c in enumerate(CLASSES)}

# --------------------------------------------------------------------------
# tools


def find_tool(name: str) -> str:
    """llvm-objdump / llvm-cxxfilt / clang / llvm-ar, preferring LLVM 22."""
    env = os.environ.get("PERRY_LLVM_BIN")
    cands = []
    if env:
        cands.append(os.path.join(env, name))
    for d in ("/usr/lib/llvm-22/bin", "/opt/homebrew/opt/llvm@22/bin",
              "/usr/local/opt/llvm@22/bin", "/opt/homebrew/opt/llvm/bin"):
        cands.append(os.path.join(d, name))
    for c in cands:
        if os.path.exists(c):
            return c
    for n in (name + "-22", name):
        w = shutil.which(n)
        if w:
            return w
    raise SystemExit(f"callgraph: cannot find {name}; set PERRY_LLVM_BIN")


# --------------------------------------------------------------------------
# rules (seeds.txt)


class Rule:
    """One line of seeds.txt: `kind match pattern -- reason`."""

    def __init__(self, kind: str, match: str, pattern: str, reason: str, line: int):
        self.targets = None
        if kind == "idiom" and " => " not in pattern:
            raise SystemExit(f"seeds.txt:{line}: an idiom rule must name its targets (=> ...)")
        if kind in ("indirect", "idiom") and " => " in pattern:
            # `indirect M P => M2 P2`: the exempted indirect call can only
            # reach functions matching (M2, P2); they become its callees, so
            # the graph -- not the reason -- decides whether they are clean.
            pattern, tgt = (x.strip() for x in pattern.split(" => ", 1))
            tm, tp = tgt.split(None, 1)
            self.targets = Rule("target", tm, tp.strip(), reason, line)
        self.kind, self.match, self.pattern, self.reason, self.line = kind, match, pattern, reason, line
        if match == "exact":
            self._f = lambda s, p=pattern: s == p
        elif match == "prefix":
            self._f = lambda s, p=pattern: s.startswith(p)
        elif match == "contains":
            self._f = lambda s, p=pattern: p in s
        elif match == "regex":
            rx = re.compile(pattern)
            self._f = lambda s, rx=rx: rx.search(s) is not None
        else:
            raise ValueError(f"seeds.txt:{line}: unknown match kind {match!r}")

    def __call__(self, s: str) -> bool:
        return self._f(s)

    def __repr__(self) -> str:
        return f"{self.kind} {self.match} {self.pattern}"


RULE_KINDS = {
    # named seeds: a symbol matching one of these is a seed of that kind
    "collector",  # starts, or runs part of, a collection (cut in AllocOnly)
    "poll",       # a declared poll: never cut
    "js",         # runs JavaScript (getter/proxy trap/coercion/closure call)
    # cuts
    "throw",      # the throw funnel (cut in ThrowOnly); must also match a node
    "panic",      # panic=abort path: never returns, cut in every class
    "teardown",   # runs only at thread/process teardown: cut in every class
    # exemptions
    "indirect",   # an indirect call inside a matching function is NOT a seed
    "extern",     # an undefined symbol matching this is a system leaf
    "forbid",     # a symbol nothing in the archives may call (a checked premise)
    "trusted",    # a Rust crate whose indirect calls are not seeds (a trust boundary)
    "idiom",      # `idiom NAME => match pattern`: a recognized indirect idiom's targets
    "closures",   # matching functions gain edges to their own `::{closure#N}` bodies
    "target",     # internal: the target side of a delegated `indirect` rule
}


def load_rules(path: str) -> list[Rule]:
    rules = []
    with open(path, encoding="utf-8") as fh:
        for n, raw in enumerate(fh, 1):
            line = raw.rstrip("\n")
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            if " -- " not in line:
                raise SystemExit(f"{path}:{n}: every rule needs ` -- <reason>`")
            head, reason = line.split(" -- ", 1)
            if len(reason.strip()) < 12:
                raise SystemExit(f"{path}:{n}: the reason is too short to be a reason")
            if head.split(None, 1)[0] == "idiom":
                # `idiom NAME => match pattern`
                parts = ["idiom", "exact", head.split(None, 1)[1]]
            else:
                parts = head.split(None, 2)
            if len(parts) != 3:
                raise SystemExit(f"{path}:{n}: expected `kind match pattern -- reason`")
            kind, match, pattern = parts[0], parts[1], parts[2].strip()
            if kind not in RULE_KINDS:
                raise SystemExit(f"{path}:{n}: unknown rule kind {kind!r}")
            rules.append(Rule(kind, match, pattern, reason.strip(), n))
    return rules


# --------------------------------------------------------------------------
# objdump parsing

MEMBER_HDR = re.compile(r"^(.*?)(?:\((.+)\))?:\s+file format (.+?)\s*$")
LABEL = re.compile(r"^([0-9a-f]+) <(.+)>:$")
INSN = re.compile(r"^\s*([0-9a-f]+):\s+(.*)$")
RELOC = re.compile(r"^\s+([0-9a-f]+):\s+(\S+)\s+(.+?)\s*$")
TARGET_ADDEND = re.compile(r"^(.*?)(?:[+-]0x[0-9a-f]+)?$")
X86_PREFIXES = {"notrack", "lock", "rep", "repe", "repne", "repz", "repnz", "bnd", "data16", "cs", "ds"}


def temp_label(name: str) -> bool:
    """Assembler-temporary labels: not function boundaries."""
    return name.startswith(("ltmp", "Ltmp", ".L", "L_", "l_", "lCPI", "LCPI", "$"))


LLVM_SUFFIX = re.compile(r"(\s*\(\.llvm\.\d+\)|\.llvm\.\d+)$")
_HASHED = re.compile(r"^(.+)-[0-9a-f]{16}$")


def crate_of(obj: str) -> str | None:
    """Rust crate of an archive member, or None for a C/assembly object.

    Staticlib members are `<lib>-<hash>.<crate>-<hash>.<crate>.<cgu>-cgu.N.rcgu.o`
    for dependencies and `<lib>-<hash>.<lib>.<cgu>-cgu.N.rcgu.o` for the
    library's own code (the `.rcgu.o` may repeat)."""
    member = obj.rsplit("/", 1)[-1].rsplit("\\", 1)[-1]
    if ".rcgu." not in member:
        return None
    parts = member.split(".")
    if len(parts) < 3:
        return None
    m = _HASHED.match(parts[1])
    return m.group(1) if m else parts[1]


class Node:
    __slots__ = ("idx", "obj", "sec", "addr", "names", "fmt", "indirect_calls",
                 "indirect_jmps", "calls", "externs", "idioms", "sites", "switches")

    def __init__(self, idx, obj, sec, addr, fmt):
        self.idx, self.obj, self.sec, self.addr, self.fmt = idx, obj, sec, addr, fmt
        self.names: list[str] = []
        self.indirect_calls = 0
        self.indirect_jmps = 0
        self.calls: set[str] = set()   # raw target names of direct branches
        self.externs: set[str] = set()
        self.idioms: set[str] = set()  # recognized indirect-call idioms (see IDIOMS)
        self.sites: list[str] = []     # "+0xOFF insn" of each unresolved indirect site
        self.switches = 0              # switch-table dispatches recognized (not seeds)


def _norm(name: str, fmt: str) -> str:
    """Canonical symbol spelling: Mach-O drops its one leading underscore,
    COFF import thunks (`__imp_foo`) name `foo`."""
    if fmt.startswith("mach-o") and name.startswith("_"):
        name = name[1:]
    if name.startswith("__imp_"):
        name = name[len("__imp_"):]
    return name


# ---- instruction-level analysis (per function, after the whole body is read)

X86_CALLER_SAVED = {"rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11"}
X86_IMPLICIT = {"rax", "rcx", "rdx", "rsi", "rdi", "r11"}
X86_IMPLICIT_MN = ("mul", "div", "imul", "idiv", "cqto", "cqo", "cltd", "cltq", "cwtl",
                   "cpuid", "rdtsc", "rdtscp", "syscall", "rep", "movs", "stos", "lods",
                   "scas", "cmps", "xgetbv", "rdrand", "rdseed", "xchg", "cmpxchg", "xadd")
A64_CALLEE_SAVED = {f"x{i}" for i in range(19, 30)}
# Registers that carry arguments at function entry, in any ABI we analyze
# (SysV x86-64, Win64, AAPCS64 incl. x8 indirect-result).
ARG_REGS = {"rdi", "rsi", "rdx", "rcx", "r8", "r9"} | {f"x{i}" for i in range(0, 9)}
X86_REG = re.compile(r"%([re]?[a-z]{1,2}x?|r\d{1,2}[dwb]?|[re]?[sd]il?|[re]?[sb]pl?|[a-d][lh])\b")
HEX_TARGET = re.compile(r"\b0x([0-9a-f]+)\b")


def x86_family(reg: str) -> str:
    reg = reg.lstrip("%")
    m = re.match(r"^r(\d{1,2})[dwb]?$", reg)
    if m:
        return "r" + m.group(1)
    base = {"al": "ax", "ah": "ax", "bl": "bx", "bh": "bx", "cl": "cx", "ch": "cx",
            "dl": "dx", "dh": "dx", "sil": "si", "dil": "di", "spl": "sp", "bpl": "bp"}.get(reg)
    if base is None:
        base = reg[1:] if reg[0] in "re" and len(reg) == 3 else reg
    return "r" + base


def a64_family(reg: str) -> str | None:
    m = re.match(r"^[xw](\d{1,2})$", reg.strip())
    return "x" + m.group(1) if m else None


class Insn:
    __slots__ = ("addr", "mn", "ops", "raw_ops", "relocs")

    def __init__(self, addr, mn, ops, raw_ops):
        self.addr, self.mn, self.ops, self.raw_ops, self.relocs = addr, mn, ops, raw_ops, []


def _x86_writes(ins: Insn) -> set[str]:
    mn, ops = ins.mn, ins.ops
    if mn.startswith("call"):
        return set(X86_CALLER_SAVED)
    if mn.startswith("j") or mn.startswith("ret"):
        return set()
    out = set()
    if mn.startswith(X86_IMPLICIT_MN):
        out |= X86_IMPLICIT
    if mn.startswith(("xchg", "cmpxchg", "xadd")):
        for m in X86_REG.finditer(re.sub(r"\([^)]*\)", "", ops)):
            out.add(x86_family(m.group(1)))
        return out
    if mn.startswith("pop"):
        m = X86_REG.search(ops)
        return out | ({x86_family(m.group(1))} if m else set())
    if (mn.startswith("cmp") and not mn.startswith(("cmps", "cmov"))) \
            or mn.startswith(("test", "push", "ucomis", "comis", "vucomis", "vcomis", "nop")) \
            or mn in ("bt", "btl", "btq", "btw"):
        return out
    # AT&T: the destination is the last operand; a register inside
    # parentheses is an address component, not a destination.
    depth, last = 0, 0
    for k, ch in enumerate(ops):
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif ch == "," and depth == 0:
            last = k + 1
    dest = ops[last:].strip()
    if dest.startswith("%"):
        m = X86_REG.match(dest)
        if m:
            out.add(x86_family(m.group(1)))
    return out


A64_NOWRITE = {"b", "br", "ret", "cbz", "cbnz", "tbz", "tbnz", "nop", "dmb", "dsb", "isb",
               "hint", "brk", "udf", "yield", "prfm", "prfum", "cmp", "cmn", "tst", "fcmp",
               "fcmpe", "ccmp", "ccmn", "fccmp", "fccmpe", "bti", "pacibsp", "autibsp",
               "paciasp", "autiasp", "retab", "retaa"}
A64_NOWRITE_PREFIX = ("b.", "str", "stp", "stur", "stlr", "st1", "st2", "st3", "st4", "stnp",
                      "sttr", "stg", "stz")


def _a64_writes(ins: Insn) -> set[str]:
    mn, ops = ins.mn, ins.ops
    if mn in ("bl", "blr") or mn.startswith("blra"):
        return {f"x{i}" for i in range(0, 19)} | {"x30"}
    out = set()
    if "]!" in ops or re.search(r"\],\s*#", ops):
        m = re.search(r"\[(x\d+|sp)", ops)
        if m and m.group(1) != "sp":
            out.add(m.group(1))
    if mn in A64_NOWRITE or mn.startswith(A64_NOWRITE_PREFIX):
        return out
    regs = [r.strip() for r in ops.split("[")[0].split(",") if r.strip()]
    nwrite = 2 if mn.startswith(("ldp", "ldnp", "ldaxp", "ldxp", "casp")) else 1
    for r in regs[:nwrite]:
        f = a64_family(r)
        if f:
            out.add(f)
    return out


def _is_switch_dispatch(insns: list[Insn], i: int, arch: str) -> bool:
    """Is the indirect JUMP at insns[i] the dispatch of a compiler-emitted
    switch table (a jump that stays inside this function)?

    x86-64 (LLVM, PIC):  movslq (%rT,%rI,4), %rX ; addq %rT, %rX ; jmpq *%rX
    x86-64 (non-PIC):    jmpq *OFF(,%rI,8) / *(%rT,%rI,8) after a table lea
    AArch64 (LLVM):      adr xB, <label> ; ldr{b,h,sw} ; add xB, xB, xO, {sxtw,lsl} #2 ; br xB

    Anything else is an indirect tail call and stays a seed.
    """
    window = insns[max(0, i - 8):i]
    ops = insns[i].ops
    if arch == "x86":
        if re.search(r"\*-?(0x[0-9a-f]+)?\((%r\w+)?,%r\w+,8\)", ops):
            return any(w.mn.startswith("lea") for w in window)
        target = ops.lstrip("*").strip()
        return (any(w.mn.startswith("add") and w.ops.endswith(target) for w in window)
                and any(w.mn.startswith("movslq") and ",4)" in w.ops for w in window))
    reg = ops.split(",")[0].strip()
    saw_add = any(w.mn == "add" and w.ops.startswith(reg + ",")
                  and ("sxtw #2" in w.ops or "lsl #2" in w.ops) for w in window)
    saw_adr = any(w.mn == "adr" and w.ops.startswith(reg + ",") for w in window)
    return saw_add and saw_adr


def _got_symbol(ins: Insn) -> str | None:
    for rtype, target in ins.relocs:
        if "GOTPCREL" in rtype or "GOT_LOAD" in rtype or "GOTPAGE" in rtype or "LD64_GOT" in rtype \
                or "ADR_GOT" in rtype or target.startswith("__imp_"):
            return target
    return None


class _Cfg:
    """Basic blocks of one function, just enough for reaching definitions."""

    def __init__(self, insns: list[Insn], arch: str, switch_at: set[int]):
        self.insns = insns
        addr_ix = {ins.addr: k for k, ins in enumerate(insns)}
        leaders = {0}
        succ_of_insn: dict[int, list[int]] = {}
        for k, ins in enumerate(insns):
            kind, target = _flow(ins, arch)
            if kind is None:
                continue
            if k + 1 < len(insns):
                leaders.add(k + 1)
            tgt = addr_ix.get(target) if target is not None else None
            if tgt is not None:
                leaders.add(tgt)
            succ_of_insn[k] = (kind, tgt)
        self.starts = sorted(leaders)
        self.block_of = [0] * len(insns)
        for b, st in enumerate(self.starts):
            en = self.starts[b + 1] if b + 1 < len(self.starts) else len(insns)
            for k in range(st, en):
                self.block_of[k] = b
        nb = len(self.starts)
        self.pred: list[set[int]] = [set() for _ in range(nb)]
        switch_blocks = set()
        for b, st in enumerate(self.starts):
            last = (self.starts[b + 1] if b + 1 < nb else len(insns)) - 1
            kind, tgt = succ_of_insn.get(last, (None, None))
            if last in switch_at:
                switch_blocks.add(b)
                continue
            if kind in (None, "call", "cond") and b + 1 < nb:
                self.pred[b + 1].add(b)          # fall through
            if kind in ("cond", "jump") and tgt is not None:
                self.pred[self.block_of[tgt]].add(b)
        # a switch table may land on any block of the function
        for b in range(nb):
            self.pred[b] |= switch_blocks


def _padding_block(cfg: "_Cfg", b: int) -> bool:
    st = cfg.starts[b]
    en = cfg.starts[b + 1] if b + 1 < len(cfg.starts) else len(cfg.insns)
    return all(cfg.insns[k].mn.startswith(("nop", "data16", "xchg")) and
               (not cfg.insns[k].mn.startswith("xchg") or cfg.insns[k].ops == "%ax, %ax")
               for k in range(st, en))


_Cfg.padding = _padding_block


def _flow(ins: Insn, arch: str):
    """(kind, local target address) for control-flow instructions."""
    mn = ins.mn
    m = HEX_TARGET.search(ins.raw_ops)
    tgt = int(m.group(1), 16) if (m and not ins.relocs and not ins.ops.startswith("*")) else None
    if arch == "x86":
        if mn.startswith("call"):
            return "call", None
        if mn.startswith("ret") or mn.startswith(("ud2", "hlt")):
            return "exit", None
        if mn.startswith("jmp"):
            return ("jump", tgt) if tgt is not None else ("exit", None)
        if mn.startswith("j"):
            return "cond", tgt
        return None, None
    if mn in ("bl", "blr") or mn.startswith("blra"):
        return "call", None
    if mn in ("ret", "retaa", "retab", "brk", "udf", "br") or mn.startswith("bra"):
        return "exit", None
    if mn == "b":
        return ("jump", tgt) if tgt is not None else ("exit", None)
    if mn.startswith(("b.", "cbz", "cbnz", "tbz", "tbnz")):
        return "cond", tgt
    return None, None


def _copy_source(ins: Insn, arch: str) -> str | None:
    """`mov %src, %dst` (x86-64, 64-bit) / `mov xD, xS` (AArch64): the source
    register family of a plain register copy, else None."""
    if arch == "x86":
        m = re.match(r"^movq\s*$", ins.mn + " ") and re.match(r"^%(r\w+), %(r\w+)$", ins.ops)
        return x86_family(m.group(1)) if m else None
    if ins.mn == "mov":
        m = re.match(r"^x(\d+), x(\d+)$", ins.ops)
        return f"x{m.group(2)}" if m else None
    return None


def _slot_load(ins: Insn, arch: str) -> str | None:
    """A reload from a frame spill slot: `movq -0x48(%rbp), %r` (x86-64) /
    `ldr xN, [sp|x29, #imm]` (AArch64). Returns the slot key."""
    if arch == "x86":
        m = ins.mn == "movq" and re.match(r"^(-?0x[0-9a-f]+\(%rbp\)), %r\w+$", ins.ops)
        return m.group(1) if m else None
    if ins.mn == "ldr":
        m = re.match(r"^x\d+, (\[(sp|x29), #-?0x[0-9a-f]+\])$", ins.ops)
        return m.group(1) if m else None
    return None


def _slot_store(ins: Insn, slot: str, arch: str):
    """(is_write, source register family) for an instruction touching `slot`."""
    if slot not in ins.ops:
        return False, None
    if arch == "x86":
        dest = ins.ops.rsplit(",", 1)[-1].strip() if "," in ins.ops else ins.ops
        if dest != slot:
            return False, None
        m = ins.mn == "movq" and re.match(r"^%(r\w+), ", ins.ops)
        return True, (x86_family(m.group(1)) if m else None)
    if ins.mn == "str":
        m = re.match(r"^x(\d+), " + re.escape(slot) + "$", ins.ops)
        return True, (f"x{m.group(1)}" if m else None)
    if ins.mn.startswith(("st", "stp")):
        return True, None
    return False, None


def _slot_pinned(cfg: "_Cfg", i: int, slot: str, arch: str, depth: int) -> str | None:
    """Every store to spill slot `slot` reaching insns[i] stores a register
    that is itself GOT-pinned to one symbol."""
    insns = cfg.insns
    sym = None
    seen: set[int] = set()
    work = [(cfg.block_of[i], i)]
    while work:
        b, upto = work.pop()
        st = cfg.starts[b]
        found = False
        for k in range(upto - 1, st - 1, -1):
            w, src = _slot_store(insns[k], slot, arch)
            if w:
                g = _got_pinned(cfg, k, src, arch, depth + 1) if src else None
                if g is None or (sym is not None and g != sym):
                    return None
                sym = g
                found = True
                break
        if found:
            continue
        if b == 0:
            continue             # slot unwritten on this path: infeasible reload
        if not cfg.pred[b]:
            if cfg.padding(b):
                continue
            return None
        for p in cfg.pred[b]:
            if p not in seen:
                seen.add(p)
                en = cfg.starts[p + 1] if p + 1 < len(cfg.starts) else len(insns)
                work.append((p, en))
    return sym


def _got_pinned(cfg: "_Cfg", i: int, reg: str, arch: str, depth: int = 0) -> str | None:
    """If every definition of `reg` reaching the indirect branch at insns[i]
    is a load of the same GOT/IAT entry, return that symbol (the branch is a
    direct call the compiler routed through a register); else None.

    Reaching definitions over the function's CFG: a call clobbers the
    caller-saved registers, reaching the function entry without a definition
    means "the caller's value", and a switch table may reach any block. Any
    definition that is not the same GOT load makes the call indirect."""
    writes = _x86_writes if arch == "x86" else _a64_writes
    insns = cfg.insns
    sym = None
    seen_blocks: set[int] = set()
    # (block, index to scan backwards from, exclusive)
    work = [(cfg.block_of[i], i)]
    while work:
        b, upto = work.pop()
        st = cfg.starts[b]
        found = False
        for k in range(upto - 1, st - 1, -1):
            ins = insns[k]
            if reg in writes(ins):
                g = _got_symbol(ins)
                if g is None and depth < 6:
                    src = _copy_source(ins, arch)
                    if src is not None and src != reg:
                        g = _got_pinned(cfg, k, src, arch, depth + 1)
                    else:
                        slot = _slot_load(ins, arch)
                        if slot is not None:
                            g = _slot_pinned(cfg, k, slot, arch, depth)
                if g is None or (sym is not None and g != sym):
                    return None
                sym = g
                found = True
                break
        if found:
            continue
        if b == 0:
            # Reaching the entry means the register holds its value at
            # function entry on this path. For an argument register that is
            # a caller-supplied function pointer: indirect. For any other
            # register a call through it would call an unknown garbage value,
            # which no compiled function does, so the path is infeasible
            # (typically a switch-table edge the CFG over-approximates).
            if reg in ARG_REGS:
                return None
            continue
        if not cfg.pred[b]:
            if cfg.padding(b):
                continue         # alignment padding after a jump: dead
            return None          # a landing pad or other unknown entry
        for p in cfg.pred[b]:
            if p not in seen_blocks:
                seen_blocks.add(p)
                en = cfg.starts[p + 1] if p + 1 < len(cfg.starts) else len(insns)
                work.append((p, en))
    return sym


def _idiom(insns: list[Insn], i: int, arch: str) -> str | None:
    """Name a recognized compiler/std indirect-call idiom at insns[i].

    io_os_functions: a call through `core::io::error::os_functions::
    OS_FUNCTIONS`, the table std installs with its errno helpers
    (decode_error_kind / error_string / is_interrupted), in a function that
    loads that table.

    io_error_drop: dropping a `std::io::Error` whose bit-packed repr holds a
    custom payload (tag 0b01). std stores the payload's owner drop function in
    the box; the inlined drop is `leaq -1(%r), %rdi ; call *0x17(%r)` behind
    an `and $3` tag test. Its only targets are
    `alloc::io::error::custom_owner_from_box::drop_box_raw::<T>`; the rule
    for it in seeds.txt makes those the callees instead of seeding.
    """
    ins = insns[i]
    if arch == "x86":
        m = re.match(r"^\*0x17\(%(\w+)\)$", ins.ops)
        if m:
            r = m.group(1)
            window = insns[max(0, i - 16):i]
            untag = any((w.mn.startswith("lea") and w.ops.startswith("-0x1(%"))
                        or (w.mn.startswith("dec") and w.ops == "%rdi") for w in window[-4:])
            tagtest = any(w.mn.startswith("and") and w.ops.startswith("$0x3,") for w in window)
            if untag and tagtest:
                return "io_error_drop"
        if re.match(r"^\*0x[0-9a-f]+\(%\w+\)$", ins.ops) and any(
                "OS_FUNCTIONS" in t for w in insns for _, t in w.relocs):
            # std's io::Error OS-function table (errno decoding): a call
            # through the table in a function that loads OS_FUNCTIONS.
            return "io_os_functions"
        return None
    # AArch64
    reg = ins.ops.split(",")[0].strip()
    window = insns[max(0, i - 16):i]
    near = window[-4:]
    m = next((re.match(rf"^{reg}, \[(x\d+), #0x17\]$", w.ops) for w in near
              if w.mn in ("ldur", "ldr") and re.match(rf"^{reg}, \[(x\d+), #0x17\]$", w.ops)), None)
    if m and any(w.mn == "sub" and w.ops.endswith(f", {m.group(1)}, #0x1") for w in near):
        # untag (tagged pointer - 1) and load the owner-drop fn at +0x18
        return "io_error_drop"
    if (ins.mn.startswith("bl")
            and any(w.mn == "ldr" and re.match(rf"^{reg}, \[{reg}, #0x[0-9a-f]+\]$", w.ops) for w in near)
            and any("OS_FUNCTIONS" in t for w in insns for _, t in w.relocs)):
        return "io_os_functions"
    return None


def _is_branch(ins: Insn, arch: str) -> bool:
    if arch == "x86":
        return ins.mn.startswith(("call", "j"))
    return ins.mn in ("b", "bl") or ins.mn.startswith(("b.", "cbz", "cbnz", "tbz", "tbnz"))


def analyze_node(node: Node, insns: list[Insn], arch: str):
    """Direct edges, indirect call/jump sites of one function."""
    fmt = node.fmt
    switch_at = {k for k, ins in enumerate(insns)
                 if ((arch == "x86" and ins.mn.startswith("jmp") and ins.ops.startswith("*")
                      and "(%rip)" not in ins.ops)
                     or (arch == "arm64" and ins.mn == "br"))
                 and _is_switch_dispatch(insns, k, arch)}
    node.switches = len(switch_at)
    cfg_box: list = []

    def cfg() -> _Cfg:
        if not cfg_box:
            cfg_box.append(_Cfg(insns, arch, switch_at))
        return cfg_box[0]

    last_tlvp = -99
    for i, ins in enumerate(insns):
        if any("TLVP" in r for r, _ in ins.relocs):
            last_tlvp = i
        if arch == "x86":
            is_call, is_jmp = ins.mn.startswith("call"), ins.mn.startswith("j")
            if not (is_call or is_jmp):
                continue
            if not ins.ops.startswith("*"):
                for _, t in ins.relocs:
                    node.calls.add(_norm(t, fmt))
                continue
            if "(%rip)" in ins.ops:
                g = _got_symbol(ins)
                if g is not None:
                    node.calls.add(_norm(g, fmt))
                    continue
            else:
                m = re.match(r"^\*%(\w+)$", ins.ops)
                if m:
                    g = _got_pinned(cfg(), i, x86_family(m.group(1)), arch)
                    if g is not None:
                        node.calls.add(_norm(g, fmt))
                        continue
            if i in switch_at:
                continue
            idiom = _idiom(insns, i, arch)
            if idiom is not None:
                node.idioms.add(idiom)
                continue
            if is_call:
                node.indirect_calls += 1
            else:
                node.indirect_jmps += 1
            node.sites.append(f"+{ins.addr:#x} {ins.mn} {ins.ops}")
            continue
        # arm64
        if _is_branch(ins, arch):
            for _, t in ins.relocs:
                node.calls.add(_norm(t, fmt))
            continue
        if ins.mn in ("blr", "br") or ins.mn.startswith(("blra", "bra")):
            reg = a64_family(ins.ops.split(",")[0].strip()) or ins.ops
            g = _got_pinned(cfg(), i, reg, arch)
            if g is not None:
                node.calls.add(_norm(g, fmt))
                continue
            if (ins.mn.startswith("bl") and i - last_tlvp <= 6
                    and any(w.mn == "ldr" and re.match(rf"^{reg}, \[x\d+\]$", w.ops)
                            for w in insns[max(0, i - 4):i])):
                # Mach-O thread-local access: `ldr x0,[TLVP]; ldr x8,[x0]; blr x8`
                # calls dyld's TLV getter, which never enters Perry code.
                node.calls.add("__tlv_get_addr")
                continue
            if i in switch_at:
                continue
            idiom = _idiom(insns, i, arch)
            if idiom is not None:
                node.idioms.add(idiom)
                continue
            if ins.mn.startswith("bl"):
                node.indirect_calls += 1
            else:
                node.indirect_jmps += 1
            node.sites.append(f"+{ins.addr:#x} {ins.mn} {ins.ops}")


def parse_disassembly(archive: str, objdump: str) -> dict:
    """Parse `llvm-objdump -d -r --show-all-symbols` of one archive."""
    cmd = [objdump, "-d", "-r", "--no-show-raw-insn", "--show-all-symbols", archive]
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True, errors="replace",
                            bufsize=1 << 20)
    nodes: list[Node] = []
    obj = sec = fmt = None
    arch = "x86"
    cur: Node | None = None
    insns: list[Insn] = []
    pending_labels: list[str] = []
    pending_addr = None

    def close():
        nonlocal insns
        if cur is not None and insns:
            analyze_node(cur, insns, arch)
        insns = []

    def open_node(addr: int):
        nonlocal cur, pending_labels
        real = [n for n in pending_labels if not temp_label(n)]
        if not real and cur is not None and cur.sec == sec and cur.obj == obj:
            # an assembler-temporary label inside a function continues it
            cur.names.extend(_norm(n, fmt) for n in pending_labels)
        else:
            close()
            cur = Node(len(nodes), obj, sec, addr, fmt)
            cur.names.extend(_norm(n, fmt) for n in pending_labels)
            nodes.append(cur)
        pending_labels = []

    for line in proc.stdout:
        if not line.strip():
            continue
        if line.startswith("\t\t"):
            m = RELOC.match(line)
            if m and insns:
                insns[-1].relocs.append((m.group(2), TARGET_ADDEND.match(m.group(3)).group(1)))
            continue
        c0 = line[0]
        if c0 == " " or c0 == "\t":
            m = INSN.match(line)
            if not m or (cur is None and not pending_labels):
                continue
            if pending_labels:
                open_node(int(m.group(1), 16))
            body = m.group(2).strip()
            parts = body.split(None, 1)
            if not parts:
                continue
            mn = parts[0]
            raw = parts[1] if len(parts) > 1 else ""
            while mn in X86_PREFIXES and raw:
                sub = raw.split(None, 1)
                mn, raw = sub[0], (sub[1] if len(sub) > 1 else "")
            ops = raw.split("//")[0].split(" # ")[0].split("<")[0].strip()
            insns.append(Insn(int(m.group(1), 16), mn, ops, raw))
            continue
        m = LABEL.match(line.rstrip("\n"))
        if m:
            addr = int(m.group(1), 16)
            if pending_labels and pending_addr != addr:
                open_node(pending_addr)   # a label with no instructions
            pending_labels.append(m.group(2))
            pending_addr = addr
            continue
        if line.startswith("Disassembly of section "):
            if pending_labels:
                open_node(pending_addr)
            close()
            sec = line[len("Disassembly of section "):].rstrip().rstrip(":")
            cur = None
            continue
        m = MEMBER_HDR.match(line)
        if m:
            if pending_labels:
                open_node(pending_addr)
            close()
            obj = m.group(2) or m.group(1)
            fmt = m.group(3)
            arch = "arm64" if ("arm64" in fmt or "aarch64" in fmt) else "x86"
            sec = None
            cur = None
            continue
    if pending_labels:
        open_node(pending_addr)
    close()
    rc = proc.wait()
    if rc != 0:
        raise SystemExit(f"callgraph: {' '.join(cmd)} exited {rc}")
    return {"nodes": nodes, "arch": arch}


NM_LINE = re.compile(r"^([0-9a-fA-F]*)\s+([A-Za-z?-])\s+(\S+)$")


def parse_globals(archive: str, nm: str) -> set[tuple[str, str]]:
    """(member, symbol) for every GLOBAL defined symbol (`llvm-nm -g`)."""
    out = subprocess.run([nm, "-g", "--defined-only", archive], capture_output=True,
                         text=True, errors="replace")
    if out.returncode != 0:
        raise SystemExit(f"callgraph: llvm-nm failed on {archive}: {out.stderr[:400]}")
    res = set()
    member = None
    for line in out.stdout.splitlines():
        if not line.strip():
            continue
        if line.endswith(":") and " " not in line.strip():
            member = line[:-1]
            if "(" in member and member.endswith(")"):
                member = member[member.index("(") + 1:-1]
            continue
        m = NM_LINE.match(line.strip())
        if m and member is not None:
            res.add((member, m.group(3)))
    return res


# --------------------------------------------------------------------------
# graph


class Graph:
    def __init__(self, nodes: list[Node], globals_: set[tuple[str, str]], fmt_of: dict):
        self.nodes = nodes
        by_obj_name: dict[tuple[str, str], list[int]] = collections.defaultdict(list)
        by_global: dict[str, list[int]] = collections.defaultdict(list)
        norm_globals = {(o, _norm(n, fmt_of.get(o, ""))) for o, n in globals_}
        by_obj_sec: dict[tuple[str, str], list[int]] = collections.defaultdict(list)
        for n in nodes:
            by_obj_sec[(n.obj, n.sec)].append(n.idx)
            for name in n.names:
                by_obj_name[(n.obj, name)].append(n.idx)
                if (n.obj, name) in norm_globals:
                    by_global[name].append(n.idx)
        self.by_global = by_global
        self.succ: list[list[int]] = [[] for _ in nodes]
        for n in nodes:
            tgt: set[int] = set()
            for t in sorted(n.calls):
                local = by_obj_name.get((n.obj, t))
                if local:
                    tgt.update(local)
                    continue
                glob = by_global.get(t)
                if glob:
                    tgt.update(glob)
                    continue
                sec = self._section_target(n, t, by_obj_sec)
                if sec is not None:
                    tgt.update(sec)
                    continue
                n.externs.add(t)
            tgt.discard(n.idx)
            self.succ[n.idx] = sorted(tgt)
        self.pred: list[list[int]] = [[] for _ in nodes]
        for a, ts in enumerate(self.succ):
            for t in ts:
                self.pred[t].append(a)
        self.edge_count = sum(len(s) for s in self.succ)
        self.dem: dict[str, str] = {}

    @staticmethod
    def _section_target(n: Node, t: str, by_obj_sec) -> list[int] | None:
        """A branch to a SECTION symbol (`call .text.foo-4`, gcc-compiled C
        calling a static function; Mach-O `__text`; COFF `.text`) targets the
        functions of that section in the same object -- all of them, since
        the addend is not a reliable function offset."""
        for cand in (t, "__TEXT," + t):
            nodes = by_obj_sec.get((n.obj, cand))
            if nodes:
                return nodes
        return None

    def demangle_all(self, cxxfilt: str):
        names = sorted({nm for n in self.nodes for nm in n.names}
                       | {e for n in self.nodes for e in n.externs})
        mangled = [x for x in names if x.startswith(("_R", "_ZN", "__ZN", "_Z"))]
        self.dem = {x: x for x in names}
        if mangled:
            out = subprocess.run([cxxfilt], input="\n".join(mangled) + "\n",
                                 capture_output=True, text=True, errors="replace")
            for a, b in zip(mangled, out.stdout.split("\n")):
                self.dem[a] = b.strip() or a

    def label(self, idx: int) -> str:
        n = self.nodes[idx]
        real = [x for x in n.names if not temp_label(x)] or n.names or [f"{n.sec}+{n.addr:#x}"]
        return " | ".join(self.dem.get(x, x) for x in real)

    def spellings(self, idx: int) -> list[str]:
        """Every spelling a rule may match: raw and demangled names."""
        n = self.nodes[idx]
        out = []
        for x in n.names:
            out.append(x)
            d = self.dem.get(x, x)
            if d != x:
                # LTO-promoted locals carry a ` (.llvm.NNN)` suffix; rules
                # match the plain name.
                out.append(LLVM_SUFFIX.sub("", d))
        return out


def build_graph(archives: list[str]) -> Graph:
    objdump = find_tool("llvm-objdump")
    nm = find_tool("llvm-nm")
    cxxfilt = find_tool("llvm-cxxfilt")
    for a in archives:
        if not os.path.exists(a):
            raise SystemExit(f"callgraph: archive not found: {a}")
    with concurrent.futures.ThreadPoolExecutor(max_workers=2 * len(archives)) as ex:
        dis = [ex.submit(parse_disassembly, a, objdump) for a in archives]
        gl = [ex.submit(parse_globals, a, nm) for a in archives]
        parsed = [f.result() for f in dis]
        globals_ = set().union(*(f.result() for f in gl))
    nodes: list[Node] = []
    fmt_of = {}
    for p in parsed:
        for n in p["nodes"]:
            n.idx = len(nodes)
            nodes.append(n)
            fmt_of[n.obj] = n.fmt
    g = Graph(nodes, globals_, fmt_of)
    g.demangle_all(cxxfilt)
    return g


# --------------------------------------------------------------------------
# classification


class Classification:
    def __init__(self):
        self.cls: dict[str, str] = {}        # exported symbol -> class
        self.node_of: dict[str, int] = {}
        self.reason: dict[int, dict[str, str]] = {}   # node -> {mode: seed reason}
        self.stats: dict = {}
        self.witness: dict[str, dict[int, int]] = {}  # mode -> node -> next hop


def _match_any(rules: list[Rule], spellings: list[str]) -> Rule | None:
    for r in rules:
        for s in spellings:
            if r(s):
                return r
    return None


def classify(g: Graph, rules: list[Rule], table_filter=None) -> Classification:
    by_kind: dict[str, list[Rule]] = collections.defaultdict(list)
    for r in rules:
        by_kind[r.kind].append(r)

    spell = [g.spellings(i) for i in range(len(g.nodes))]
    named: dict[int, tuple[str, Rule]] = {}
    hits: dict[int, int] = collections.Counter()   # rule line -> node hits
    for i in range(len(g.nodes)):
        for kind in ("poll", "js", "collector", "throw", "panic", "teardown"):
            r = _match_any(by_kind[kind], spell[i])
            if r is not None:
                hits[r.line] += 1
                named.setdefault(i, (kind, r))
    dead = [r for r in rules if r.kind in ("poll", "js", "collector", "throw", "panic", "teardown")
            and hits[r.line] == 0]

    exempt_ind: dict[int, Rule] = {}
    trusted_ind: set[int] = set()
    extern_ok: dict[str, Rule] = {}
    crate_trust: dict[str, Rule | None] = {}
    for i, n in enumerate(g.nodes):
        if n.indirect_calls or n.indirect_jmps:
            r = _match_any(by_kind["indirect"], spell[i])
            crate = crate_of(n.obj) if r is None else None
            if r is not None:
                exempt_ind[i] = r
                hits[r.line] += 1
            elif crate is not None:
                if crate not in crate_trust:
                    crate_trust[crate] = _match_any(by_kind["trusted"], [crate])
                t = crate_trust[crate]
                if t is not None:
                    trusted_ind.add(i)
                    hits[t.line] += 1
        for e in n.externs:
            if e not in extern_ok:
                r = _match_any(by_kind["extern"], [e, g.dem.get(e, e)])
                if r is not None:
                    extern_ok[e] = r
                    hits[r.line] += 1

    # Delegated exemptions: the exempted function gains edges to every
    # function its rule names as a possible target.
    extra_pred: dict[int, list[int]] = collections.defaultdict(list)
    delegated = [r for r in by_kind["indirect"] if r.targets is not None]
    if delegated:
        tmatch = {r.line: [i for i in range(len(g.nodes)) if _match_any([r.targets], spell[i])]
                  for r in delegated}
        for i, r in sorted(exempt_ind.items()):
            if r.targets is not None:
                for t in tmatch[r.line]:
                    if t != i:
                        extra_pred[t].append(i)

    # `forbid` rules are premises other reasons rest on (e.g. "Perry never
    # registers a mimalloc hook"): any direct reference fails the run.
    forbidden = []
    for r in by_kind["forbid"]:
        for i, n in enumerate(g.nodes):
            for t in sorted(n.calls):
                if r(t) or r(g.dem.get(t, t)):
                    forbidden.append(f"seeds.txt:{r.line}: {g.label(i)[:120]} calls {t}")
                    hits[r.line] += 1

    # `closures` rules: a function that hands its own closure to a dyn-FnMut
    # dispatcher (Once::call) gets that closure as an explicit callee.
    if by_kind["closures"]:
        by_prefix: dict[str, list[int]] = collections.defaultdict(list)
        for t in range(len(g.nodes)):
            for sp in spell[t]:
                k = sp.find("::{closure#")
                if k > 0:
                    by_prefix[sp[:k]].append(t)
        for i in range(len(g.nodes)):
            r = _match_any(by_kind["closures"], spell[i])
            if r is None:
                continue
            hits[r.line] += 1
            for sp in spell[i]:
                for t in by_prefix.get(sp, ()):
                    if t != i:
                        extra_pred[t].append(i)

    idiom_rules = {r.pattern: r for r in by_kind["idiom"]}
    unknown_idiom: dict[int, str] = {}
    for rl in idiom_rules.values():
        tm = [t for t in range(len(g.nodes)) if _match_any([rl.targets], spell[t])]
        for i, n in enumerate(g.nodes):
            if rl.pattern in n.idioms:
                hits[rl.line] += 1
                for t in tm:
                    if t != i:
                        extra_pred[t].append(i)
    for i, n in enumerate(g.nodes):
        for name in sorted(n.idioms):
            if name not in idiom_rules:
                unknown_idiom[i] = name

    # AArch64 machine-outlined fragments (OUTLINED_FUNCTION_N) are pieces of
    # their callers: an indirect branch in one is exempt exactly when every
    # function that calls it would be.
    def rule_exempt(i: int) -> bool:
        if _match_any(by_kind["indirect"], spell[i]) is not None:
            return True
        crate = crate_of(g.nodes[i].obj)
        return crate is not None and crate_trust.get(crate, _match_any(by_kind["trusted"], [crate])) is not None

    for i, n in enumerate(g.nodes):
        if (n.indirect_calls or n.indirect_jmps) and i not in exempt_ind and i not in trusted_ind \
                and any(nm.startswith("OUTLINED_FUNCTION_") for nm in n.names) \
                and g.pred[i] and all(rule_exempt(c) for c in g.pred[i]):
            trusted_ind.add(i)

    def base_seed(i: int) -> str | None:
        kn = named.get(i)
        if kn and kn[0] in ("poll", "js"):
            return f"{kn[0]}:{kn[1].pattern}"
        n = g.nodes[i]
        if i in unknown_idiom:
            return "idiom_without_rule:" + unknown_idiom[i]
        if (n.indirect_calls or n.indirect_jmps) and i not in exempt_ind and i not in trusted_ind:
            return "indirect_call" if n.indirect_calls else "indirect_jump"
        bad_ext = sorted(e for e in n.externs if e not in extern_ok)
        if bad_ext:
            return "extern:" + bad_ext[0]
        return None

    base = [base_seed(i) for i in range(len(g.nodes))]

    def solve(mode: str) -> tuple[dict[int, str], dict[int, int]]:
        """Reverse BFS from seeds; cut nodes neither seed nor propagate."""
        def is_cut(i: int) -> bool:
            kn = named.get(i)
            if kn is None:
                return False
            k = kn[0]
            if k in ("panic", "teardown"):
                return True
            if mode in ("L2b", "L2b_throw") and k == "collector":
                return True
            if mode == "L2b_throw" and k == "throw":
                return True
            return False

        bad: dict[int, str] = {}
        nxt: dict[int, int] = {}
        work = collections.deque()
        for i in range(len(g.nodes)):
            if is_cut(i):
                continue
            s = base[i]
            if s is None and mode == "L2":
                kn = named.get(i)
                if kn and kn[0] == "collector":
                    s = f"collector:{kn[1].pattern}"
            if s is not None:
                bad[i] = s
                work.append(i)
        while work:
            t = work.popleft()
            for c in (g.pred[t] + extra_pred[t]) if t in extra_pred else g.pred[t]:
                if c in bad or is_cut(c):
                    continue
                bad[c] = "via"
                nxt[c] = t
                work.append(c)
        return bad, nxt

    res = Classification()
    b2, n2 = solve("L2")
    b2b, n2b = solve("L2b")
    b2t, n2t = solve("L2b_throw")
    res.witness = {"L2": n2, "L2b": n2b, "L2b_throw": n2t}
    res.reason = {"L2": b2, "L2b": b2b, "L2b_throw": b2t}

    exported: dict[str, int] = {}
    for i, n in enumerate(g.nodes):
        for name in n.names:
            if name in g.by_global and i in g.by_global[name]:
                if table_filter is None or table_filter(n.obj, name):
                    exported.setdefault(name, i)
    for name in sorted(exported):
        i = exported[name]
        if i not in b2:
            c = "Leaf"
        elif i not in b2b:
            c = "AllocOnly"
        elif i not in b2t:
            c = "ThrowOnly"
        else:
            c = "Reenters"
        res.cls[name] = c
        res.node_of[name] = i
    res.stats = {
        "nodes": len(g.nodes),
        "edges": g.edge_count,
        "indirect_call_nodes": sum(1 for n in g.nodes if n.indirect_calls),
        "indirect_jump_nodes": sum(1 for n in g.nodes if n.indirect_jmps),
        "exempted_indirect_nodes": len(exempt_ind),
        "trusted_indirect_nodes": len(trusted_ind),
        "named_seed_nodes": dict(collections.Counter(k for k, _ in named.values())),
        "unresolved_externs": sorted({e for n in g.nodes for e in n.externs if e not in extern_ok}),
        "classes": dict(collections.Counter(res.cls.values())),
        "dead_rules": [f"seeds.txt:{r.line}: {r}" for r in dead],
        "forbidden_calls": forbidden,
        "unused_exemptions": [f"seeds.txt:{r.line}: {r}" for r in rules
                              if r.kind in ("indirect", "extern") and hits[r.line] == 0],
    }
    return res


def witness_path(g: Graph, res: Classification, name: str, mode: str) -> list[str]:
    """The BFS-shortest path from SYMBOL to the seed that taints it."""
    i = res.node_of.get(name)
    if i is None:
        return [f"{name}: not an exported symbol of these archives"]
    bad, nxt = res.reason[mode], res.witness[mode]
    if i not in bad:
        return [f"{name}: reaches no seed in mode {mode}"]
    out = []
    seen = set()
    while i is not None and i not in seen:
        seen.add(i)
        why = bad.get(i, "?")
        if why.startswith("indirect") and g.nodes[i].sites:
            why += " @" + g.nodes[i].sites[0]
        out.append(f"{why:>28}  {g.label(i)[:200]}")
        i = nxt.get(i)
    return out


# --------------------------------------------------------------------------
# table I/O

TABLE_HEADER = """\
# GENERATED by scripts/gc_call_effects/callgraph.py generate -- do not edit by hand.
# target: {target}
# archives: {archives}
# Regenerate: scripts/gc_call_effects/regen.sh {target}   (or take the CI artifact)
# Classes (RFC deferred collection): Leaf < AllocOnly < ThrowOnly < Reenters.
# Codegen reads every target's file and uses the most conservative class.
# Known reclassifications kept here on purpose (the graph proves them):
#   #11522 js_array_length / js_object_alloc_class_inline_keys* /
#          js_object_get_own_field_or_undef reach JS or a collector -> not Leaf.
#   #11523 CannotCollect helpers whose GcRootRegistryGuard drop flushes a
#          deferred collection reach the collector -> not Leaf.
# symbol\tclass
"""


def perry_member(obj: str, name: str) -> bool:
    """Only symbols from Perry's own crates are tabled; C dependencies
    (mimalloc, zstd, ...) are never called by codegen by name."""
    crate = crate_of(obj)
    if crate is None or not crate.startswith("perry"):
        return False
    return not name.startswith(("_R", "_ZN", "_Z", "__")) and not temp_label(name)


def write_table(res: Classification, target: str, archives: list[str], out: str):
    lines = [TABLE_HEADER.format(target=target,
                                 archives=" ".join(os.path.basename(a) for a in archives))]
    for name in sorted(res.cls):
        lines.append(f"{name}\t{res.cls[name]}\n")
    tmp = out + ".tmp"
    with open(tmp, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("".join(lines))
    os.replace(tmp, out)


def read_table(path: str) -> dict[str, str]:
    out = {}
    with open(path, encoding="utf-8") as fh:
        for n, line in enumerate(fh, 1):
            if line.startswith("#") or not line.strip():
                continue
            parts = line.rstrip("\n").split("\t")
            if len(parts) != 2 or parts[1] not in CLASS_RANK:
                raise SystemExit(f"{path}:{n}: malformed table row {line!r}")
            if parts[0] in out:
                raise SystemExit(f"{path}:{n}: duplicate symbol {parts[0]}")
            out[parts[0]] = parts[1]
    return out


def compare(committed: dict[str, str], fresh: dict[str, str]):
    """(unsafe, safe) drift.

    Unsafe: the committed class is WEAKER (closer to Leaf) than what the
    archives prove -- codegen would omit a statepoint the runtime needs. A
    symbol absent from a table reads as Reenters (that is what codegen does
    with an unknown callee), so a new symbol is safe drift and a vanished one
    that was committed below Reenters is unsafe (its class is now unproven).
    """
    unsafe, safe = [], []
    for name in sorted(set(committed) | set(fresh)):
        c, f = committed.get(name), fresh.get(name)
        ceff, feff = c or "Reenters", f or "Reenters"
        row = (name, c or "(absent)", f or "(absent)")
        if CLASS_RANK[ceff] < CLASS_RANK[feff]:
            unsafe.append(row)
        elif c != f:
            safe.append(row)
    return unsafe, safe


# --------------------------------------------------------------------------
# CLI


def run_classification(archives: list[str], rules_path: str, strict_rules: bool = True):
    rules = load_rules(rules_path)
    g = build_graph(archives)
    res = classify(g, rules, table_filter=perry_member)
    if res.stats["forbidden_calls"]:
        raise SystemExit("callgraph: a `forbid` premise is violated -- an exemption's "
                         "reason no longer holds:\n  " + "\n  ".join(res.stats["forbidden_calls"][:20]))
    if strict_rules and res.stats["dead_rules"]:
        raise SystemExit("callgraph: named seed/cut rules that match NO node -- a seed that "
                         "seeds nothing makes every verdict below it vacuous:\n  "
                         + "\n  ".join(res.stats["dead_rules"]))
    return g, res


def print_stats(res: Classification, out=sys.stderr):
    s = res.stats
    print(f"[gc-call-effects] nodes={s['nodes']} edges={s['edges']} "
          f"indirect_call_nodes={s['indirect_call_nodes']} indirect_jump_nodes={s['indirect_jump_nodes']} "
          f"exempted_indirect={s['exempted_indirect_nodes']} trusted_indirect={s['trusted_indirect_nodes']} named_seeds={s['named_seed_nodes']}", file=out)
    print(f"[gc-call-effects] classes={dict(sorted(s['classes'].items()))} "
          f"unresolved_externs={len(s['unresolved_externs'])}", file=out)
    if s["unused_exemptions"]:
        print("[gc-call-effects] exemptions that matched nothing on this target (fine on "
              "another target, stale if on none):\n  " + "\n  ".join(s["unused_exemptions"]), file=out)


def cmd_generate(a):
    g, res = run_classification(a.archives, a.rules)
    print_stats(res)
    if len(res.cls) < a.min_symbols:
        raise SystemExit(f"callgraph: only {len(res.cls)} exported symbols classified "
                         f"(< {a.min_symbols}) -- wrong archives, or the parser broke")
    write_table(res, a.target, a.archives, a.out)
    print(f"[gc-call-effects] wrote {a.out} ({len(res.cls)} symbols)", file=sys.stderr)
    return 0


def cmd_check(a):
    g, res = run_classification(a.archives, a.rules)
    print_stats(res)
    if len(res.cls) < a.min_symbols:
        print(f"callgraph: only {len(res.cls)} exported symbols classified "
              f"(< {a.min_symbols}) -- wrong archives, or the parser broke", file=sys.stderr)
        return 1
    committed = read_table(a.table)
    unsafe, safe = compare(committed, res.cls)
    if a.write_fresh:
        write_table(res, a.target, a.archives, a.write_fresh)
    rc = 0
    if unsafe:
        rc = 1
        print(f"\nUNSAFE: {len(unsafe)} symbol(s) whose committed class is weaker than the "
              f"archives prove (codegen would skip a statepoint the runtime needs):", file=sys.stderr)
        for name, c, f in unsafe[:60]:
            print(f"  {name}: committed {c}, archives say {f}", file=sys.stderr)
            if f != "(absent)":
                mode = {"Leaf": "L2", "AllocOnly": "L2b", "ThrowOnly": "L2b_throw"}[c]
                for step in witness_path(g, res, name, mode)[:12]:
                    print(f"      {step}", file=sys.stderr)
    if safe:
        rc = 1
        print(f"\nDRIFT: {len(safe)} symbol(s) differ in the safe direction (new symbols, new "
              f"leaves, or already-conservative changes). Regenerate {a.table}:", file=sys.stderr)
        for name, c, f in safe[:40]:
            print(f"  {name}: committed {c}, archives say {f}", file=sys.stderr)
        if len(safe) > 40:
            print(f"  ... {len(safe) - 40} more", file=sys.stderr)
    if rc == 0:
        print(f"[gc-call-effects] {a.table}: {len(committed)} symbols, identical to the archives",
              file=sys.stderr)
    return rc


def cmd_why(a):
    g, res = run_classification(a.archives, a.rules)
    for name in a.symbols:
        print(f"== {name}: {res.cls.get(name, 'not exported')}")
        for mode in ("L2", "L2b", "L2b_throw"):
            print(f"  [{mode}]")
            for step in witness_path(g, res, name, mode):
                print(f"    {step}")
    return 0


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv and argv[0] == "--self-test":
        sys.path.insert(0, HERE)
        import self_test  # noqa: E402
        return self_test.main()
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--rules", default=os.path.join(HERE, "seeds.txt"))
    sub = p.add_subparsers(dest="cmd", required=True)
    pg = sub.add_parser("generate")
    pg.add_argument("--target", required=True)
    pg.add_argument("--out", required=True)
    pg.add_argument("--min-symbols", type=int, default=1000)
    pg.add_argument("archives", nargs="+")
    pc = sub.add_parser("check")
    pc.add_argument("--target", required=True)
    pc.add_argument("--table", required=True)
    pc.add_argument("--write-fresh", help="also write the regenerated table here")
    pc.add_argument("--min-symbols", type=int, default=1000)
    pc.add_argument("archives", nargs="+")
    pw = sub.add_parser("why")
    pw.add_argument("--archive", dest="archives", action="append", required=True)
    pw.add_argument("symbols", nargs="+")
    a = p.parse_args(argv)
    return {"generate": cmd_generate, "check": cmd_check, "why": cmd_why}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
