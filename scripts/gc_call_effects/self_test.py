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
extern void _Unwind_RaiseException(void *);
void fixture_poll(void) { fixture_collect(); }
void fixture_collect_js(void) { js_proxy_get(0); }
extern void *memcpy(void *, const void *, unsigned long);

int leaf_helper(int x) { PLANT return x * 3 + 1; }
int js_back_hot_text(int x) { PLANT return x; }
int alloc_helper(int x) { fixture_collect(); return x + 2; }
int throw_helper(int x) { if (x > 100) js_throw(); return x; }
int reenter_helper(int x) { return js_proxy_get(x) + 3; }
int poll_helper(int x) { fixture_poll(); return x + 4; }
int gc_reaches_js_helper(int x) { fixture_collect_js(); return x + 6; }
int unwind_helper(int x) { _Unwind_RaiseException(0); return x + 8; }
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
poll exact fixture_poll -- fixture stand-in for a declared GC poll
unwind exact _Unwind_RaiseException -- fixture stand-in for an unwinding raise
extern regex ^(memcpy|_Unwind_RaiseException)$ -- libc memory primitive / unwinder, no callbacks
"""

# A collector body that reaches JS: cut in every solve, `throws` included.
RULES_COLLECTOR_JS = RULES + "collector exact fixture_collect_js -- fixture collector that runs JS\n"

EXPECTED = {
    "js_back_hot_text": "Leaf",
    "leaf_helper": "Leaf",
    "libc_helper": "Leaf",
    "switch_helper": "Leaf",
    "alloc_helper": "AllocOnly",
    "throw_helper": "ThrowOnly",
    "reenter_helper": "Reenters",
    "chain_helper": "Reenters",
    "indirect_helper": "Reenters",
    "extern_helper": "Reenters",
    "poll_helper": "Reenters",
    "unwind_helper": "Reenters",
}

# The `throws` column. A poll is Reenters but cannot throw; the funnel, JS, an
# unaudited indirect or extern callee, and an unwinding raise all can.
EXPECTED_THROWS = {
    "js_back_hot_text": "no",
    "leaf_helper": "no",
    "libc_helper": "no",
    "switch_helper": "no",
    "alloc_helper": "no",
    "poll_helper": "no",
    "throw_helper": "yes",
    "reenter_helper": "yes",
    "chain_helper": "yes",
    "indirect_helper": "yes",
    "extern_helper": "yes",
    "unwind_helper": "yes",
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
    # cc::Build's native member convention, as used for src/hot_text.c.
    obj = os.path.join(tmp, "8a1b2c3d-hot_text.o")
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


def classify_native_hot_text(tmp: str, triple: str, tag: str) -> str | None:
    """Analyze the production C body, including the non-Linux no-op body."""
    obj = os.path.join(tmp, "9abcdef0-hot_text.o")
    lib = os.path.join(tmp, f"libhot_text_{tag}.a")
    subprocess.run([cg.find_tool("clang"), "-target", triple, "-ffreestanding",
                    "-O3", "-ffunction-sections", "-c",
                    os.path.join(cg.ROOT, "crates", "perry-runtime", "src", "hot_text.c"),
                    "-o", obj], check=True)
    subprocess.run([cg.find_tool("llvm-ar"), "rcs", lib, obj], check=True)
    graph = cg.build_graph([lib])
    result = cg.classify(graph, cg.load_rules(os.path.join(HERE, "seeds.txt")),
                         table_filter=cg.perry_member)
    return result.cls.get("js_back_hot_text")


def load_rules_text(tmp: str, text: str) -> list:
    path = os.path.join(tmp, "rules.txt")
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(text)
    return cg.load_rules(path)


def io_error_drop_cases() -> list[str]:
    """The io::Error custom-payload drop idiom in its Win64 spelling (#12479).

    The exact instructions are js_checkpoint's on x86_64-pc-windows-msvc (the
    `Stderr::write_fmt` result drop): tag dispatch on the low two bits, then
    `movq %rax, %rcx ; decq %rcx ; callq *0x17(%rax)`. In a COFF object that
    is the idiom (its targets come from the `idiom io_error_drop` rule); every
    near miss stays a seeded indirect call."""
    failures = []

    def site(fmt, mov="movq %rax, %rcx", dec="decq %rcx", call="*0x17(%rax)", tag=True):
        body = [("testq", "%rax, %rax"), ("je", "0x68"), ("movl", "%eax, %ecx")]
        if tag:
            body.append(("andl", "$0x3, %ecx"))
        body += [("leal", "-0x2(%rcx), %edx"), ("cmpl", "$0x2, %edx"), ("jb", "0x68"),
                 ("testq", "%rcx, %rcx"), ("je", "0x68"),
                 tuple(mov.split(" ", 1)), tuple(dec.split(" ", 1)), ("callq", call),
                 ("retq", "")]
        insns = [cg.Insn(0x48 + 4 * k, mn, ops, ops) for k, (mn, ops) in enumerate(body)]
        node = cg.Node(0, "fixture.o", ".text", 0, fmt)
        node.names = ["io_drop_site"]
        cg.analyze_node(node, insns, "x86")
        return node

    n = site("coff-x86-64")
    if n.idioms != {"io_error_drop"} or n.indirect_calls:
        failures.append(f"Win64 io::Error drop not recognized: idioms={n.idioms} "
                        f"indirect={n.indirect_calls}")
    near_misses = {
        "ELF (dec %rcx is not the SysV box register)": site("elf64-x86-64"),
        "no `and $3` tag test": site("coff-x86-64", tag=False),
        "%rcx copied from another register": site("coff-x86-64", mov="movq %rbx, %rcx"),
        "untag in a non-argument register": site("coff-x86-64", mov="movq %rax, %rdx",
                                                 dec="decq %rdx"),
        "a slot other than the owner-drop fn": site("coff-x86-64", call="*0x1f(%rax)"),
    }
    for why, n in near_misses.items():
        if n.idioms or n.indirect_calls != 1:
            failures.append(f"io_error_drop near miss ({why}) was not a seed: "
                            f"idioms={n.idioms} indirect={n.indirect_calls}")
    return failures


def got_pinning_cases() -> list[str]:
    """A GOT relocation on a call names its callee, not the call's return value."""
    failures = []

    def cfg(insns):
        return cg._Cfg(insns, "x86", set())

    # Linux PIC can call dlsym through its GOT entry. The return in %rax is
    # the looked-up function pointer, so a later indirect call must stay a
    # seed rather than becoming a direct edge to dlsym.
    dlsym_call = cg.Insn(0, "callq", "*0x0(%rip)", "*0x0(%rip)")
    dlsym_call.relocs = [("R_X86_64_GOTPCREL", "dlsym")]
    later_call = cg.Insn(1, "callq", "*%rax", "*%rax")
    resolved = cg._got_pinned(cfg([dlsym_call, later_call]), 1, "rax", "x86")
    if resolved is not None:
        failures.append(f"call through a GOT-pinned dlsym result resolved as {resolved}")

    # A genuine load of a function address from the GOT remains a resolved
    # direct edge; this is the negative control for the call-clobber check.
    got_load = cg.Insn(0, "movq", "0x0(%rip), %rax", "0x0(%rip), %rax")
    got_load.relocs = [("R_X86_64_GOTPCREL", "plugin_deactivate")]
    direct_call = cg.Insn(1, "callq", "*%rax", "*%rax")
    resolved = cg._got_pinned(cfg([got_load, direct_call]), 1, "rax", "x86")
    if resolved != "plugin_deactivate":
        failures.append(f"genuine GOT-pinned call resolved as {resolved!r}")
    return failures


def main() -> int:
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="gc-call-effects-selftest-") as tmp:
        rules = load_rules_text(tmp, RULES)
        alias_rule = load_rules_text(tmp, "indirect-all regex ^audited_ -- only the audited initializer aliases are native leaves\n")
        node = cg.Node(0, "fixture.o", ".text", 0, "elf64-x86-64")
        node.names = ["audited_initializer"]
        graph = cg.Graph([node], set(), {"fixture.o": node.fmt})
        if cg._match_indirect(alias_rule, graph, 0) is None:
            failures.append("audited initializer was not admitted")
        # An unrelated monomorphization folded at the same address must
        # invalidate an exemption even though its first alias still matches.
        node.names.append("unrelated_initializer")
        if cg._match_indirect(alias_rule, graph, 0) is not None:
            failures.append("a folded unrelated alias inherited an audited exemption")
        # COFF section-symbol aliases are not callable aliases: they must not
        # defeat an exemption, nor grant one, nor drop Rust `$u20$` names.
        audited = "audited_initializer"
        for sec in (".text$unlikely", ".text", ".rdata$zzz", ".xdata$x"):
            node.names = [audited, sec]
            if cg._match_indirect(alias_rule, graph, 0) is None:
                failures.append(f"section alias {sec} defeated an audited exemption")
            if not cg.temp_label(sec):
                failures.append(f"{sec} was not treated as a section symbol")
        node.names = [".text$unlikely"]
        if cg._match_indirect(alias_rule, graph, 0) is not None:
            failures.append("a bare section symbol was admitted as an audited alias")
        node.names = ["unrelated_initializer", ".text$unlikely"]
        if cg._match_indirect(alias_rule, graph, 0) is not None:
            failures.append("non-exempt indirect call became exempt via a section alias")
        for keep in ("_ZN4core3ptr$u20$drop$LT$T$GT$17h0123456789abcdefE", "foo$bar", "text$unlikely"):
            if cg.temp_label(keep):
                failures.append(f"{keep} was wrongly dropped as a section symbol")
        node.names = [audited, "_ZN4core3ptr$u20$x17h0123456789abcdefE"]
        if cg._match_indirect(alias_rule, graph, 0) is not None:
            failures.append("a $u20$ Rust alias was dropped instead of blocking the exemption")
        failures += io_error_drop_cases()
        failures += got_pinning_cases()
        per_format = {}
        for tag, triple in TARGETS.items():
            if classify_native_hot_text(tmp, triple, tag) != "Leaf":
                failures.append(f"{tag}: production js_back_hot_text is not a tabled Leaf")
            lib = build_archive(tmp, triple, tag, plant=False)
            g, res = classify(lib, rules)
            native = cg.classify(g, rules, table_filter=cg.perry_member)
            if native.cls.get("js_back_hot_text") != "Leaf":
                failures.append(f"{tag}: repository-owned C export missing from table")
            if cg.perry_member("8a1b2c3d-mimalloc.o", "js_back_hot_text"):
                failures.append(f"{tag}: a dependency inherited native ownership by name")
            got = {k: res.cls.get(k, "(missing)") for k in EXPECTED}
            got_throws = {k: res.throws.get(k, "(missing)") for k in EXPECTED_THROWS}
            per_format[tag] = (got, got_throws)
            for sym, want in EXPECTED_THROWS.items():
                if got_throws[sym] != want:
                    failures.append(f"{tag}: {sym} throws={got_throws[sym]}, expected {want}")
            if cg.agreement_violations(res):
                failures.append(f"{tag}: Leaf/AllocOnly with throws=yes: {cg.agreement_violations(res)}")
            # A collector whose body reaches JS is cut in `throws` (the
            # collection-never-unwinds premise), exactly as a collector is cut
            # in AllocOnly: the helper stays AllocOnly and throws=no.
            _, res_cj = classify(lib, load_rules_text(tmp, RULES_COLLECTOR_JS))
            if (res_cj.cls["gc_reaches_js_helper"], res_cj.throws["gc_reaches_js_helper"]) != ("AllocOnly", "no"):
                failures.append(f"{tag}: collector reaching JS: gc_reaches_js_helper is "
                                f"{res_cj.cls['gc_reaches_js_helper']}/throws={res_cj.throws['gc_reaches_js_helper']}, "
                                f"expected AllocOnly/no")
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
            native_planted = cg.classify(g2, rules, table_filter=cg.perry_member)
            unsafe_native, _ = cg.compare(native.cls, native_planted.cls)
            if not any(n == "js_back_hot_text" for n, _, _ in unsafe_native):
                failures.append(f"{tag}: collecting native export was not UNSAFE drift")
            if res2.cls.get("leaf_helper") != "Reenters":
                failures.append(f"{tag}: planted js_proxy_get call left leaf_helper "
                                f"{res2.cls.get('leaf_helper')} -- the checker cannot fail")
            unsafe, _ = cg.compare(res.cls, res2.cls)
            if not any(name == "leaf_helper" for name, _, _ in unsafe):
                failures.append(f"{tag}: table check did not report the planted helper as UNSAFE")
            path = cg.witness_path(g2, res2, "leaf_helper", "L2")
            if not path or "js_proxy_get" not in path[-1]:
                failures.append(f"{tag}: witness path does not end at js_proxy_get: {path}")
            if res2.throws.get("leaf_helper") != "yes":
                failures.append(f"{tag}: planted js_proxy_get call left leaf_helper throws=no")
            unsafe_t, _ = cg.compare_throws(res.throws, res2.throws)
            if not any(name == "leaf_helper" for name, _, _ in unsafe_t):
                failures.append(f"{tag}: throws check did not report the planted helper as UNSAFE")
            tpath = cg.witness_path(g2, res2, "leaf_helper", "throws")
            if not tpath or "js_proxy_get" not in tpath[-1]:
                failures.append(f"{tag}: throws witness does not end at js_proxy_get: {tpath}")

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

    fake = cg.Classification()
    fake.cls = {"a": "Leaf", "b": "AllocOnly", "c": "Reenters"}
    fake.throws = {"a": "yes", "b": "no", "c": "yes"}
    if cg.agreement_violations(fake) != ["a"]:
        failures.append(f"agreement_violations() cannot fail: {cg.agreement_violations(fake)}")

    unsafe, safe = cg.compare_throws({"a": "no", "b": "yes", "d": "no"}, {"b": "no", "c": "no", "d": "yes"})
    if [u[0] for u in unsafe] != ["a", "d"] or [x[0] for x in safe] != ["b", "c"]:
        failures.append(f"compare_throws() semantics broke: unsafe={unsafe} safe={safe}")

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
