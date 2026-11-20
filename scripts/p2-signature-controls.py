#!/usr/bin/env python3
"""P2 typed signatures (DESIGN-p2 rev 2.3 §5): IR assertions for the controls.

Compiles tests/parity/language/function/test_p2_signature_{functions,methods}.ts
with PERRY_SAVE_LL and checks the IR shape each control names: which bodies
get an `F$sig`, its signature, and which functions call it. Exit 0 when every
assertion holds; 1 otherwise, printing each failure.

    scripts/p2-signature-controls.py <perry release dir> [--keep DIR]

Run it with the fact on (the default). `PERRY_FACTS_TYPED_SIGNATURES=0` must
make every `defines`/`calls` assertion of a `$sig` fail, which
`--expect-off` checks instead (no `$sig` symbol at all).
"""
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONTROLS = "tests/parity/language/function"

DEFINE = re.compile(r"^define [^@]*@(\S+?)\((.*?)\)")
CALL = re.compile(r"call [^@]*@([A-Za-z0-9_$.]+)\(")

# (file, kind, subject, object). Symbols match by suffix after the module
# prefix, so `encode$sig` is `perry_fn_<module>__encode$sig`.
ASSERTIONS = [
    # S1: encode is (i32, i32, i32, i32) -> i32, entered from the top level.
    ("functions", "signature", "encode$sig", "i32 (i32, i32, i32, i32)"),
    ("functions", "calls", "main", "encode$sig"),
    # S8b: recursion enters the typed body.
    ("functions", "calls", "down$sig", "down$sig"),
    # S2: every caller passes a fraction: F64, and the sites enter `$sig`.
    ("functions", "signature", "twice$sig", "double (double)"),
    ("functions", "calls", "main", "twice$sig"),
    # S21 (precision selector): an F64 signature does not subsume main's
    # I32 spec tier; `half` keeps it.
    ("functions", "no-define", "half$sig", None),
    ("functions", "define", "half$spec_i32", None),
    # E3a / E3b: N/A as join controls (refused via Boxed params): the String
    # makes the parameter Boxed, so there is no signature to refuse.
    ("functions", "no-define", "e3a$sig", None),
    ("functions", "no-define", "e3bInner$sig", None),
    # E3c: `e3cSmall` is inlined into main (its second site), so its call of
    # `e3cInner` has copies in main and in its own body; they join, and both
    # enter `e3cInner$sig`.
    ("functions", "calls", "main", "e3cInner$sig"),
    ("functions", "calls", "e3cSmall", "e3cInner$sig"),
    # S26: the recursive typed body is entered, and is a named JS frame
    # (`sig_frames`).
    ("functions", "calls", "s26_depth$sig", "s26_depth$sig"),
    # T4d: typed_i1 is an excluded tier.
    ("functions", "no-define", "isOdd$sig", None),
    # S6: the site holds two bodies (T0Set): neither is typed.
    ("methods", "no-define", "A6__v$sig", None),
    ("methods", "no-define", "B6__v$sig", None),
    # S19: a typed method beside `.call` with another layout.
    ("methods", "define", "P19__sum$sig", None),
    # S25 (§3.2.1 tower: composes): one receiver class enters `M$sig`; a
    # receiver that may be the overriding subclass keeps the dispatch.
    ("methods", "define", "C25__m$sig", None),
    ("methods", "calls", "typed25", "C25__m$sig"),
    ("methods", "no-call", "mixed25", "C25__m$sig"),
    ("methods", "no-call", "mixed25", "D25__m$sig"),
    ("methods", "calls", "mixed25", "D25__m"),
    # E3j: both arms of a for…of over a plain array enter `M$sig`.
    ("methods", "define", "Q3__f$sig", None),
    ("methods", "calls", "main", "Q3__f$sig"),
]


def functions(ll_text):
    """{name: (header params, set of callees)} for every defined function."""
    out = {}
    cur = None
    for line in ll_text.splitlines():
        m = DEFINE.match(line)
        if m:
            cur = m.group(1)
            ret = line.split("@", 1)[0].split()[-1]
            params = ", ".join(
                p.strip().split()[0] for p in m.group(2).split(",") if p.strip()
            )
            out[cur] = (f"{ret} ({params})", set())
            continue
        if cur is None:
            continue
        if line.startswith("}"):
            cur = None
            continue
        for c in CALL.findall(line):
            out[cur][1].add(c)
    return out


def find(fns, suffix):
    """Defined functions whose name is `suffix` after the module prefix."""
    if suffix == "main":
        return [n for n in fns if n == "main"]
    return [n for n in fns if n.endswith("__" + suffix)]


def bodies(fns, name):
    """Every body of function `name`: its public symbol and its `$` tiers."""
    if name == "main":
        return ["main"]
    return [n for n in fns if n.endswith("__" + name) or ("__" + name + "$") in n]


def compile_control(perry, name, keep):
    src = os.path.join(ROOT, CONTROLS, f"test_p2_signature_{name}.ts")
    ll = os.path.join(keep, name)
    os.makedirs(ll, exist_ok=True)
    env = dict(os.environ)
    env.update(
        PERRY_RUNTIME_DIR=perry,
        PERRY_NO_CACHE="1",
        PERRY_SAVE_LL=ll,
    )
    r = subprocess.run(
        [os.path.join(perry, "perry"), "compile", src, "-o", os.path.join(ll, "bin")],
        env=env,
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        sys.exit(f"compile {name} failed:\n{r.stdout}{r.stderr}")
    text = "".join(
        open(os.path.join(ll, f)).read() for f in sorted(os.listdir(ll)) if f.endswith(".ll")
    )
    return functions(text), text


def sig_frames(text):
    """Failures of the frame rule: a typed body is its function's frame, so
    (as main's spec clones) it has a function record (`$info`) and, on x86_64
    Linux, lives in the JS text section the `Error` capture reads."""
    linux = sys.platform.startswith("linux") and os.uname().machine == "x86_64"
    out = []
    for line in text.splitlines():
        m = DEFINE.match(line)
        if not m or not m.group(1).strip('"').endswith("$sig"):
            continue
        name = m.group(1).strip('"')
        if linux and 'section "perry_js_text"' not in line:
            out.append(f"{name} is not in perry_js_text")
        if f"@\"{name}$info\" =" not in text and f"@{name}$info =" not in text:
            out.append(f"{name} has no $info record")
    return out


def check(fns, kind, subject, obj):
    defs = find(fns, subject)
    if kind == "define":
        return bool(defs), f"{subject} is defined"
    if kind == "no-define":
        return not defs, f"{subject} is not defined"
    if kind == "signature":
        got = [fns[d][0] for d in defs]
        return got == [obj], f"{subject} has type {obj} (got {got})"
    defs = bodies(fns, subject)
    callees = set().union(*(fns[d][1] for d in defs)) if defs else set()
    hit = any(c.endswith("__" + obj) for c in callees)
    if kind == "calls":
        return bool(defs) and hit, f"{subject} calls {obj}"
    if kind == "no-call":
        return bool(defs) and not hit, f"{subject} does not call {obj}"
    raise ValueError(kind)


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    perry = os.path.abspath(args[0])
    expect_off = "--expect-off" in args
    keep = args[args.index("--keep") + 1] if "--keep" in args else tempfile.mkdtemp()
    failures = 0
    for name in ("functions", "methods"):
        fns, text = compile_control(perry, name, keep)
        if expect_off:
            sigs = sorted(n for n in fns if n.endswith("$sig"))
            ok = not sigs
            print(("ok  " if ok else "FAIL"), name, "has no $sig symbol", sigs[:5])
            failures += not ok
            continue
        for file, kind, subject, obj in ASSERTIONS:
            if file != name:
                continue
            ok, what = check(fns, kind, subject, obj)
            print(("ok  " if ok else "FAIL"), name, what)
            failures += not ok
        bad = sig_frames(text)
        print(("ok  " if not bad else "FAIL"), name, "every $sig body is a named JS frame", bad[:5])
        failures += bool(bad)
    print(f"p2-signature-controls: {failures} failure(s); IR in {keep}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
