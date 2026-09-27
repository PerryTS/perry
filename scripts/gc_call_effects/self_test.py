#!/usr/bin/env python3
"""Self-test for callgraph.py: prove the classifier can say "no".

Compiles a small C fixture to REAL object files for all three archive formats
the CI checker reads (ELF x86-64, Mach-O arm64, COFF x86-64), archives them,
and runs the production pipeline (llvm-objdump parsing, graph, fixed points,
table comparison) over them. It asserts:

  * every class is produced, and identically on every format;
  * a switch table is not mistaken for an indirect call, a GOT/IAT call is a
    direct edge, and a genuine function-pointer call is a seed;
  * SABOTAGE: planting a `js_proxy_get` call into a Leaf helper turns it
    Reenters, and the table check reports it as UNSAFE drift (the #11522
    shape) -- so a green real run is evidence, not an absence of checking;
  * a named seed that matches nothing, and a violated `forbid` premise, both
    fail the run instead of passing vacuously.

Run: python3 scripts/gc_call_effects/callgraph.py --self-test
Needs clang, llvm-ar, llvm-objdump, llvm-nm and llvm-cxxfilt (LLVM 22).
"""
from __future__ import annotations

import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import callgraph as cg  # noqa: E402

FIXTURE = r"""
typedef int (*fp)(int);
volatile int fixture_sink;
int js_proxy_get(int x) { fixture_sink = x; return x ^ 0x5a; }
void fixture_collect(void) { __asm__ volatile("" ::: "memory"); }
void js_throw(void) { js_proxy_get(1); }
extern int unknown_external(int);
extern void *memcpy(void *, const void *, unsigned long);

int leaf_helper(int x) { PLANT return x * 3 + 1; }
int alloc_helper(int x) { fixture_collect(); return x + 2; }
int throw_helper(int x) { if (x > 100) js_throw(); return x; }
int reenter_helper(int x) { return js_proxy_get(x) + 3; }
int chain_helper(int x) { return reenter_helper(x) * 5; }
int indirect_helper(fp f, int x) { return f(x) + 7; }
int extern_helper(int x) { return unknown_external(x) + 11; }
int libc_helper(char *d, const char *s, unsigned long n) { memcpy(d, s, n); return 0; }
int switch_helper(int k) {
    switch (k) {
    case 0: return leaf_helper(k);
    case 1: fixture_sink = 48; return 18;
    case 2: fixture_sink = 85; return 31;
    case 3: fixture_sink = 122; return 44;
    case 4: fixture_sink = 159; return 57;
    case 5: fixture_sink = 196; return 70;
    case 6: fixture_sink = 233; return 83;
    case 7: fixture_sink = 270; return 96;
    case 8: fixture_sink = 307; return 109;
    case 9: fixture_sink = 344; return 122;
    case 10: fixture_sink = 381; return 135;
    case 11: fixture_sink = 418; return 148;
    case 12: fixture_sink = 455; return 161;
    case 13: fixture_sink = 492; return 174;
    case 14: fixture_sink = 529; return 187;
    case 15: fixture_sink = 566; return 200;
    case 16: fixture_sink = 603; return 213;
    case 17: fixture_sink = 640; return 226;
    case 18: fixture_sink = 677; return 239;
    case 19: fixture_sink = 714; return 252;
    default: return 0;
    }
}
"""

RULES = """\
collector exact fixture_collect -- fixture stand-in for the allocation trigger
js exact js_proxy_get -- fixture stand-in for a JS-invoking entry point
throw exact js_throw -- fixture stand-in for the throw funnel
extern regex ^memcpy$ -- libc memory primitive, no callbacks
"""

EXPECTED = {
    "leaf_helper": "Leaf",
    "libc_helper": "Leaf",
    "switch_helper": "Leaf",
    "alloc_helper": "AllocOnly",
    "throw_helper": "ThrowOnly",
    "reenter_helper": "Reenters",
    "chain_helper": "Reenters",
    "indirect_helper": "Reenters",
    "extern_helper": "Reenters",
}

TARGETS = {
    "elf-x86_64": "x86_64-unknown-linux-gnu",
    "macho-arm64": "arm64-apple-macos11",
    "coff-x86_64": "x86_64-pc-windows-msvc",
}


def build_archive(tmp: str, triple: str, tag: str, plant: bool) -> str:
    clang = cg.find_tool("clang")
    ar = cg.find_tool("llvm-ar")
    src = os.path.join(tmp, f"fixture_{tag}.c")
    with open(src, "w", encoding="utf-8") as fh:
        fh.write(FIXTURE.replace("PLANT", "x = js_proxy_get(x);" if plant else ""))
    obj = os.path.join(tmp, f"fixture_{tag}.o")
    lib = os.path.join(tmp, f"libfixture_{tag}.a")
    subprocess.run([clang, "-target", triple, "-O2", "-ffunction-sections", "-fno-inline",
                    "-fno-builtin", "-c", src, "-o", obj], check=True)
    if os.path.exists(lib):
        os.remove(lib)
    subprocess.run([ar, "rcs", lib, obj], check=True)
    return lib


def classify(lib: str, rules: list) -> dict:
    g = cg.build_graph([lib])
    res = cg.classify(g, rules)
    return g, res


def load_rules_text(tmp: str, text: str) -> list:
    path = os.path.join(tmp, "rules.txt")
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(text)
    return cg.load_rules(path)


def main() -> int:
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="gc-call-effects-selftest-") as tmp:
        rules = load_rules_text(tmp, RULES)
        per_format = {}
        for tag, triple in TARGETS.items():
            lib = build_archive(tmp, triple, tag, plant=False)
            g, res = classify(lib, rules)
            got = {k: res.cls.get(k, "(missing)") for k in EXPECTED}
            per_format[tag] = got
            for sym, want in EXPECTED.items():
                if got[sym] != want:
                    failures.append(f"{tag}: {sym} is {got[sym]}, expected {want}")
            sw = [n for n in g.nodes if "switch_helper" in n.names]
            if not sw or sw[0].switches == 0:
                failures.append(f"{tag}: switch_helper has no recognized switch table -- the "
                                f"switch case of the fixture is vacuous")
            if res.stats["dead_rules"]:
                failures.append(f"{tag}: unexpected dead rules {res.stats['dead_rules']}")

            # SABOTAGE: plant the #11522 shape into the Leaf helper.
            planted = build_archive(tmp, triple, tag + "-planted", plant=True)
            g2, res2 = classify(planted, rules)
            if res2.cls.get("leaf_helper") != "Reenters":
                failures.append(f"{tag}: planted js_proxy_get call left leaf_helper "
                                f"{res2.cls.get('leaf_helper')} -- the checker cannot fail")
            unsafe, _ = cg.compare(res.cls, res2.cls)
            if not any(name == "leaf_helper" for name, _, _ in unsafe):
                failures.append(f"{tag}: table check did not report the planted helper as UNSAFE")
            path = cg.witness_path(g2, res2, "leaf_helper", "L2")
            if not path or "js_proxy_get" not in path[-1]:
                failures.append(f"{tag}: witness path does not end at js_proxy_get: {path}")

            # A seed that seeds nothing must fail, not pass vacuously.
            dead_rules = load_rules_text(tmp, RULES + "js exact no_such_symbol_anywhere -- a "
                                         "deliberately dead seed for the self-test\n")
            _, res3 = classify(lib, dead_rules)
            if not res3.stats["dead_rules"]:
                failures.append(f"{tag}: a seed matching nothing was not reported dead")

            # A violated forbid premise must be reported.
            forbid_rules = load_rules_text(tmp, RULES + "forbid exact unknown_external -- a "
                                           "deliberately violated premise for the self-test\n")
            _, res4 = classify(lib, forbid_rules)
            if not res4.stats["forbidden_calls"]:
                failures.append(f"{tag}: a violated forbid premise was not reported")

        base = per_format["elf-x86_64"]
        for tag, got in per_format.items():
            if got != base:
                failures.append(f"{tag} disagrees with elf-x86_64: {got} vs {base}")

    # Table-drift semantics: missing symbol reads as Reenters.
    unsafe, safe = cg.compare({"a": "Leaf", "b": "Reenters"}, {"b": "Leaf", "c": "Leaf"})
    if [u[0] for u in unsafe] != ["a"] or sorted(x[0] for x in safe) != ["b", "c"]:
        failures.append(f"compare() semantics broke: unsafe={unsafe} safe={safe}")

    if failures:
        print("gc_call_effects self-test FAILED:", file=sys.stderr)
        for f in failures:
            print("  " + f, file=sys.stderr)
        return 1
    print(f"gc_call_effects self-test passed: {len(EXPECTED)} fixture helpers x "
          f"{len(TARGETS)} object formats, planted #11522 shape caught, dead seed and "
          f"forbid premise enforced", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
