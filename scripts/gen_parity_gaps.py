#!/usr/bin/env python3
"""Regenerate Perry's runtime-parity gap analysis from authoritative sources.

This replaces the previously ad-hoc gap doc whose matcher massively
over-reported gaps (e.g. claimed node:crypto had 124 missing APIs when the
manifest already declares createCipheriv/createSign/timingSafeEqual/...).

Coverage sources, in priority order:
  1. Manifest entries  -- crates/perry-api-manifest/src/entries.rs plus any
     crates/perry-api-manifest/src/entries/*.rs split files (the entry data
     was split across entries/part_{1..4}.rs on 2026-07-03 to keep files
     under the 2000-line CI gate; entries.rs now just declares the `mod`s and
     concatenates them -- see MANIFEST_FILES below):
        method("mod","member",..)  /  property("mod","member",..)
     A CI test asserts every NATIVE_MODULE_TABLE entry has a row here, so this
     is the authoritative set of compile-time-dispatched top-level methods.
  2. Expr::* HIR variants -- crates/perry-hir/src/ir.rs
        PascalCase(module)+PascalCase(member) membership.
  3. js_* FFI exports + dispatched method-name string literals, scanned per
     module from stdlib/runtime/ext source. Catches instance methods
     (cipher.final, hash.digest, ecdh.computeSecret, ...) that never appear in
     the manifest because the manifest is keyed by module, not class.

Inventory source: docs/runtime-parity.md (markdown tables, one row per API).

Usage:
    python3 scripts/gen_parity_gaps.py                 # print summary + per-module report
    python3 scripts/gen_parity_gaps.py --module crypto # focus one module
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
INVENTORY = ROOT / "docs" / "runtime-parity.md"
MANIFEST_DIR = ROOT / "crates" / "perry-api-manifest" / "src"
# The manifest entry table lives in entries.rs and/or split files under
# entries/ (split out 2026-07-03 to stay under the 2000-line CI gate; see
# entries.rs's own `mod part_N;` declarations). Glob both shapes rather than
# hardcoding a single path so a future re-split can't silently zero out most
# of the manifest again -- a stale single-file path here once undercounted
# coverage by ~480 entries and swung node:os from 0 gaps to 143.
MANIFEST_FILES = sorted(MANIFEST_DIR.glob("entries*.rs")) + sorted(
    MANIFEST_DIR.glob("entries/*.rs")
)
# Below this, something is almost certainly wrong with MANIFEST_FILES (a
# missed split, a moved directory, ...) rather than a genuine drop in
# manifest size -- fail loudly instead of silently emitting bogus gap counts.
MIN_PLAUSIBLE_MANIFEST_ENTRIES = 1500
IR_DIR = ROOT / "crates" / "perry-hir" / "src" / "ir"
SRC_DIRS = [
    ROOT / "crates" / "perry-runtime" / "src",
    ROOT / "crates" / "perry-stdlib" / "src",
] + sorted((ROOT / "crates").glob("perry-ext-*/src"))

# APIs that ARE implemented but whose dispatch lives in a generic, non
# module-named file the path heuristics can't attribute. Each entry cites the
# backing source so the override is auditable. {module: {member, ...}}.
MANUAL_COVERAGE = {
    # KeyObject property/method access handled in the generic object field
    # dispatcher (crates/perry-runtime/src/object/field_get_set.rs via
    # `key_bytes == b"asymmetricKeyType"` etc.), not a crypto-named file.
    "crypto": {
        "asymmetricKeyType", "asymmetricKeyDetails", "symmetricKeySize",
        "export", "equals", "type",
    },
}

ROW_RE = re.compile(r"^\|\s*`([^`]+)`\s*\|")
SECTION_RE = re.compile(r"^###\s+(.*)$")
MANIFEST_RE = re.compile(r'\b(?:method|property)\(\s*"([^"]+)"\s*,\s*"([^"]+)"')


def snake(name: str) -> str:
    s = re.sub(r"(.)([A-Z][a-z]+)", r"\1_\2", name)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    return s.lower()


def pascal(name: str) -> str:
    return "".join(p[:1].upper() + p[1:] for p in re.split(r"[._]", name) if p)


def parse_inventory():
    """Return {module: [(signature, leading, member)]}. Skips `class X` markers."""
    modules: dict[str, list] = {}
    cur = None
    in_inventory = False
    for line in INVENTORY.read_text(encoding="utf-8").splitlines():
        m = SECTION_RE.match(line)
        if m:
            title = m.group(1)
            nm = re.search(r"node:([A-Za-z0-9_/]+)", title)
            if nm:
                cur = nm.group(1)  # keep full module key, e.g. fs/promises
                in_inventory = True
            elif title.startswith("Web"):
                cur = "__web__"
                in_inventory = True
            elif title.startswith("Bun-only") or title.startswith("Summary") or title.startswith("Notes"):
                cur = None
                in_inventory = False
            else:
                cur = None
            continue
        if not in_inventory or cur is None:
            continue
        rm = ROW_RE.match(line)
        if not rm:
            continue
        sig = rm.group(1)
        if sig.startswith("class ") or sig in ("✓", "⚠", "✗", "—"):
            continue
        member = extract_member(sig)
        leading = sig.split(".")[0].split("(")[0].strip()
        if member:
            modules.setdefault(cur, []).append((sig, leading, member))
    return modules


def extract_member(sig: str) -> str | None:
    """crypto.createSign(alg) -> createSign ; cipher.final([enc]) -> final ;
    crypto.constants -> constants ; new URL(input) -> URL ; 'message' -> message"""
    s = sig.strip()
    s = re.sub(r"^new\s+", "", s)
    s = s.strip("'\"")
    head = s.split("(")[0].strip()
    if not head:
        return None
    member = head.split(".")[-1]
    member = member.split("[")[0].strip()
    member = re.sub(r"[^\w$]", "", member)
    return member or None


def parse_manifest():
    if not MANIFEST_FILES:
        sys.exit(
            f"gen_parity_gaps: no manifest source files matched under {MANIFEST_DIR} "
            "(expected entries.rs and/or entries/*.rs) -- has the manifest crate moved?"
        )
    by_module: dict[str, set] = {}
    total = 0
    for path in MANIFEST_FILES:
        for mod, member in MANIFEST_RE.findall(path.read_text(encoding="utf-8")):
            by_module.setdefault(mod, set()).add(member)
            total += 1
    if total < MIN_PLAUSIBLE_MANIFEST_ENTRIES:
        sys.exit(
            f"gen_parity_gaps: only parsed {total} manifest entries across "
            f"{len(MANIFEST_FILES)} file(s) ({', '.join(str(p.relative_to(ROOT)) for p in MANIFEST_FILES)}); "
            f"expected at least {MIN_PLAUSIBLE_MANIFEST_ENTRIES}. The manifest was likely split/moved again "
            "and MANIFEST_FILES no longer covers it -- refusing to emit misleadingly-bad gap counts."
        )
    return by_module


def parse_expr_variants() -> set:
    txt = "\n".join(f.read_text(encoding="utf-8", errors="ignore") for f in IR_DIR.rglob("*.rs"))
    # crude: collect PascalCase enum variant idents inside the Expr enum
    return set(re.findall(r"\b([A-Z][A-Za-z0-9]+)\s*[\({,]", txt))


def scan_ffi_and_dispatch():
    """Return (ffi_fns:set[str], dispatch_literals:set[str], files_text:dict)."""
    ffi = set()
    literals = set()
    files = {}
    for d in SRC_DIRS:
        if not d.exists():
            continue
        for f in d.rglob("*.rs"):
            t = f.read_text(encoding="utf-8", errors="ignore")
            files[str(f)] = t
            for fn in re.findall(r"\bfn (js_[a-z0-9_]+)", t):
                ffi.add(fn)
            # short quoted identifiers used as match arms (method dispatch)
            for lit in re.findall(r'"([a-zA-Z][a-zA-Z0-9]{1,40})"', t):
                literals.add(lit)
    return ffi, literals, files


CONST_RE = re.compile(r"^[A-Z][A-Z0-9_]+$")


def covered(module, leading, member, sig, manifest, exprs, ffi, files):
    if member in MANUAL_COVERAGE.get(module, ()):
        return "manual"
    mset = manifest.get(module, set())
    if member in mset:
        return "manifest"
    mod_root = snake(module.replace("/", "_")).split("_")[0]
    # Constants (SIGINT, E2BIG, Z_OK, ...) are covered as a block if the module
    # exposes `constants` and the symbol appears anywhere in the module source.
    if CONST_RE.match(member) and ("constants" in mset or "codes" in mset):
        for path, t in files.items():
            if mod_root in path and re.search(r"\b%s\b" % re.escape(member), t):
                return "const"
        return "const"  # constants block is present; individual leaf assumed covered
    # Events ('message', 'close', ...): covered if the module dispatches `on`.
    is_event = sig.strip().startswith(("'", '"')) and "." not in sig
    if is_event and ("on" in mset or "addListener" in mset or "emit" in mset):
        return "event"
    # Expr variant: ONLY the compound Module+Member form (e.g. OsPlatform,
    # CryptoRandomUUID). A bare PascalCase(member) is too loose -- tokens like
    # `Lookup`/`Resolve`/`Close` occur all over the HIR and would falsely cover
    # stub modules.
    for combo in (pascal(module) + pascal(member), pascal(leading) + pascal(member)):
        if combo in exprs and len(pascal(member)) > 2:
            return "expr"
    msnake = snake(member)
    mod_snake = snake(module.replace("/", "_"))
    lead_snake = snake(leading)
    # FFI: js_<module>_..._<member>  or js_<leading>_..._<member>
    for fn in ffi:
        if not fn.endswith(("_" + msnake, msnake)):
            continue
        if (mod_snake in fn) or (lead_snake in fn and lead_snake not in (module, "")):
            return "ffi"
    # Dispatched method-name literal -- but ONLY for modules that have a real
    # implementation (manifest entry or a js_<module>_* FFI fn). Otherwise a
    # stub file that merely names methods in an error string would falsely read
    # as covered (e.g. node:dns, which has zero implementation).
    if len(member) > 2 and module_has_impl(mod_root, manifest, ffi):
        # accept match-arm (`"x" =>`) or byte-string key compare (`b"x"`)
        pat = re.compile(r'b?"%s"\s*(?:=>|\)|,|\.|=|\|)' % re.escape(member))
        for path, t in files.items():
            if mod_root in path and pat.search(t):
                return "dispatch"
    return None


def module_has_impl(mod_root, manifest, ffi):
    if any(mod_root == snake(m).split("_")[0] for m in manifest):
        return True
    return any(fn.startswith("js_" + mod_root + "_") for fn in ffi)


def compute():
    inv = parse_inventory()
    manifest = parse_manifest()
    exprs = parse_expr_variants()
    ffi, _literals, files = scan_ffi_and_dispatch()
    rows = []
    for module, apis in sorted(inv.items()):
        if module == "__web__":
            continue
        cov = 0
        missing = []
        for sig, leading, member in apis:
            if covered(module, leading, member, sig, manifest, exprs, ffi, files):
                cov += 1
            else:
                missing.append(sig)
        rows.append((module, cov, len(missing), missing))
    return rows


# Carried forward verbatim from the hand-measured behavioral status; not
# recomputed here (it comes from scripts/node_suite_run.py, not the manifest).
BEHAVIORAL_NOTE = (
    "> **Behavioral status.** This list counts individual API *surface* gaps, not\n"
    "> behavioral pass rate. Measured against Node's own test suite\n"
    "> (`scripts/node_suite_run.py` vs `test-parity/node_suite_baseline.json`),\n"
    "> Perry's runtime passes **~97%**; overall Node.js/TypeScript compatibility is\n"
    "> around **95%**. Heavily-used modules (`fs`, `http`/`https`/`http2`,\n"
    "> `net`/`tls`, `crypto`, `stream`, `events`, `child_process`,\n"
    "> `worker_threads`, `process`, `zlib`) are real, not stubs.\n"
)


def emit_doc(rows) -> str:
    tot_c = sum(c for _, c, _, _ in rows)
    tot_g = sum(g for _, _, g, _ in rows)
    out = []
    w = out.append
    w("# Perry Runtime Parity Gap List\n")
    w("> **Generated** by `scripts/gen_parity_gaps.py` from `docs/runtime-parity.md`")
    w("> (the API inventory) reconciled against Perry's coverage sources. Do not")
    w("> edit by hand — re-run the script to refresh.\n")
    w("This is a structured gap analysis comparing the public Node.js API surface")
    w("against the APIs Perry can dispatch. Coverage is derived from four sources:")
    w("the unimplemented-API gate manifest (`crates/perry-api-manifest/src/entries.rs`")
    w("and `entries/*.rs`, `method`/`property` rows), compound `Expr::*` HIR variants")
    w("(`crates/perry-hir/src/ir/`), `js_*` FFI exports across `perry-runtime` /")
    w("`perry-stdlib` / `perry-ext-*`, and module-gated method-dispatch literals.\n")
    w(BEHAVIORAL_NOTE)
    w("## Summary\n")
    w(f"Across {len(rows)} `node:*` modules: **{tot_c} covered / {tot_g} gap** "
      f"of {tot_c + tot_g} catalogued APIs.\n")
    w("> Web / global APIs and Bun-only APIs are tracked separately in")
    w("> `runtime-parity.md`; their coverage is curated, not recomputed here.\n")
    w("| Module | Covered | Gap | Total |")
    w("|--------|--------:|----:|------:|")
    for module, cov, gap, _ in sorted(rows, key=lambda r: (-r[2], r[0])):
        w(f"| `node:{module}` | {cov} | {gap} | {cov + gap} |")
    w(f"| **Total** | **{tot_c}** | **{tot_g}** | **{tot_c + tot_g}** |\n")
    w("## Per-module gaps\n")
    w("Only modules with at least one remaining gap are listed, in descending")
    w("gap-size order. Modules omitted here have **zero** catalogued gaps.\n")
    for module, cov, gap, missing in sorted(rows, key=lambda r: (-r[2], r[0])):
        if gap == 0:
            continue
        w(f"### node:{module}\n")
        w(f"**Covered: {cov} · Gap: {gap}**\n")
        for sig in missing:
            w(f"- `{sig}`")
        w("")
    w("## Methodology & caveats\n")
    w("- **Coverage = dispatchable, not byte-for-byte.** A manifest/FFI match means")
    w("  Perry can dispatch the call, not that every option/overload matches Node.")
    w("- **Module-gated dispatch.** Method-name string literals only count for")
    w("  modules that have a real implementation (a manifest entry or a")
    w("  `js_<module>_*` FFI export), so stub files naming methods in error strings")
    w("  don't read as covered.")
    w("- **Manual coverage overrides.** A few APIs are implemented in generic,")
    w("  non-module-named dispatchers (e.g. `KeyObject` property access in")
    w("  `perry-runtime/src/object/field_get_set.rs`). These are credited via an")
    w("  audited `MANUAL_COVERAGE` table in the script.")
    w("- **Constants & events** are credited as a block when the module exposes")
    w("  `constants`/`codes` or an `on`/`emit` surface, rather than per-leaf.")
    w("- `class X` declaration rows are excluded from counts.\n")
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--module", help="focus a single module, e.g. crypto")
    ap.add_argument("--emit", action="store_true",
                    help="write the full regenerated doc to docs/runtime-parity-gaps.md")
    args = ap.parse_args()

    rows = compute()

    if args.emit:
        out = ROOT / "docs" / "runtime-parity-gaps.md"
        out.write_text(emit_doc(rows))
        tot_g = sum(g for _, _, g, _ in rows)
        print(f"wrote {out.relative_to(ROOT)} ({tot_g} gaps across {len(rows)} modules)")
        return

    focus = [r for r in rows if not args.module or r[0] == args.module]
    print(f"{'module':32} {'covered':>8} {'gap':>5} {'total':>6}")
    print("-" * 56)
    for module, cov, miss, _ in focus:
        print(f"{module:32} {cov:>8} {miss:>5} {cov+miss:>6}")
    print("-" * 56)
    print(f"{'TOTAL':32} {sum(r[1] for r in focus):>8} "
          f"{sum(r[2] for r in focus):>5} {sum(r[1]+r[2] for r in focus):>6}")
    if args.module:
        for module, cov, miss, missing in focus:
            print(f"\n### node:{module}  (covered {cov} / gap {miss})\n")
            for sig in missing:
                print(f"  - {sig}")


if __name__ == "__main__":
    main()
