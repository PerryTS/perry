#!/usr/bin/env python3
"""Ratchet the places that unbox a string value as if it were a heap pointer.

Since #10762 short strings (up to ``SHORT_STRING_MAX_LEN`` bytes) are stored
inline in the NaN-box under ``SHORT_STRING_TAG`` (0x7FF9). There is no
``StringHeader`` behind such a value, so the classic unboxing
``(bits & POINTER_MASK) as *const StringHeader`` turns its characters into an
address: a segfault, or garbage. Code that first checks for ``STRING_TAG``
(0x7FFF) avoids the crash but then treats the short string as "not a string"
and returns a wrong answer (#11430, #11519).

Three source shapes are counted per crate:

* ``mask-cast``: a function in a runtime-side crate that casts masked bits to
  ``*const/*mut StringHeader`` and contains no sign of handling the inline
  representation at all (no ``is_short_string``, ``is_any_string``,
  ``SHORT_STRING``/``0x7FF9``, or a call to one of the SSO-aware accessors).
  Many such functions are correct because their input can never be SSO -- keys
  read back out of an object's keys array, internal caches -- which is why this
  is a ratchet over debt rather than a ban. New code should use
  ``str_bytes_from_jsvalue``/``with_string_value_bytes`` (borrow, no
  allocation), ``js_ffi_arg_ptr``/``perry_ffi::string_arg_ptr`` (a native that
  only reads during the call), ``JsValue::to_owned_string`` (ext crates), or
  ``js_get_string_pointer_unified`` (a heap copy).
* ``heap-tag-only``: a function in a runtime-side crate that reads a
  ``StringHeader`` behind a heap-only string test -- ``== STRING_TAG``,
  ``== 0x7FFF``, ``.is_string()``, ``.as_string_ptr()``,
  ``js_nanbox_get_string_pointer`` -- with no SSO marker anywhere in its body.
  Such a function does not crash on a short string; it silently treats it as
  "not a string" (``new Date("20" + "20")`` was an Invalid Date). One finding
  per function.
* ``codegen-str-arg``: a perry-codegen call that passes the result of
  ``unbox_to_i64`` (a bare mask) to a runtime entry whose parameter at that
  position is declared ``*const/*mut StringHeader``. Operands loaded from a
  string-literal handle global are exempt: literals are always heap strings.
  Use ``unbox_ffi_str_arg`` / ``unbox_ffi_str_arg_inline`` / ``unbox_str_handle``.

The committed baseline is debt, not an allowance for new code. A category may
never increase in a crate, and a decrease must lower the baseline in the same
change. New crates implicitly start at zero.

Usage:
    python3 scripts/sso_unbox_inventory.py
    python3 scripts/sso_unbox_inventory.py --self-test
    python3 scripts/sso_unbox_inventory.py --write-baseline
    python3 scripts/sso_unbox_inventory.py --list
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
import tempfile
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASELINE = REPO_ROOT / "scripts" / "sso_unbox_baseline.txt"
RULES = ("mask-cast", "heap-tag-only", "codegen-str-arg")

# Reuse the Rust comment/string masker and brace matcher of the sibling
# payload-access ratchet rather than keeping a second copy in sync.
_SPEC = importlib.util.spec_from_file_location(
    "string_payload_access_inventory",
    Path(__file__).resolve().parent / "string_payload_access_inventory.py",
)
_PAYLOAD = importlib.util.module_from_spec(_SPEC)
assert _SPEC.loader is not None
sys.modules[_SPEC.name] = _PAYLOAD
_SPEC.loader.exec_module(_PAYLOAD)
mask_non_code = _PAYLOAD.mask_non_code
matching_brace = _PAYLOAD.matching_brace

MASK = (
    r"(?:POINTER_MASK|PTR_MASK|PAYLOAD_MASK|NANBOX_PTR_MASK|"
    r"0x0000_FFFF_FFFF_FFFF|0x0000_ffff_ffff_ffff|0x0000FFFFFFFFFFFF|"
    r"0xFFFF_FFFF_FFFF\b)"
)
MASK_CAST_RE = re.compile(
    MASK + r"[^;{}]*?\bas\s+\*\s*(?:const|mut)\s+(?:[A-Za-z_][A-Za-z0-9_]*::)*StringHeader\b"
)
SSO_MARKER_RE = re.compile(
    r"is_short_string|is_any_string|SHORT_STRING|0x7FF9|0x7ff9|short_string_to_buf|"
    r"str_bytes_from_jsvalue|str_bytes_ascii_from_jsvalue|with_string_value_bytes|"
    r"js_get_string_pointer_unified|js_ffi_arg_ptr|string_arg_ptr|to_owned_string|"
    r"js_string_materialize_to_heap|js_value_to_str_ptr_for_ffi"
)
FUNCTION_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:<[^{;]*?>)?\s*\(", re.MULTILINE)
HEAP_TAG_RE = re.compile(
    r"[=!]=\s*(?:crate::value::|perry_runtime::value::)?(?:STRING_TAG|0x7FFF|0x7fff)\b|"
    r"(?:STRING_TAG|0x7FFF)\s*=>|\.is_string\s*\(\s*\)|\.as_string_ptr\s*\(\s*\)|"
    r"js_nanbox_get_string_pointer\s*\("
)
TEST_MOD_RE = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")
TEST_FN_RE = re.compile(r"#\s*\[\s*test\s*\]")
EXTERN_FN_RE = re.compile(
    r'extern\s+"C(?:-unwind)?"\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)', re.S
)
# A codegen call: `"js_name", &[ (TY, &a), (TY, &b), ... ]`.
CALL_RE = re.compile(r'"([A-Za-z_][A-Za-z0-9_]*)"\s*,\s*&\[(.*?)\]\s*,?\s*\)', re.S)
UNBOX_LET_RE = re.compile(
    r"let\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::\s*[A-Za-z0-9_]+\s*)?=\s*"
    r"(?:crate::expr::|super::|helpers::)*unbox_to_i64\s*\(\s*[^,]+,\s*&\s*([A-Za-z_][A-Za-z0-9_.\[\]]*)\s*\)"
)
LITERAL_LOAD_RE_TMPL = r"let\s+{name}\s*=\s*[^;]*?\.load\s*\(\s*DOUBLE\s*,\s*&[^;]*?global"


@dataclass(frozen=True)
class Finding:
    crate: str
    rel_path: str
    line_no: int
    rule: str
    detail: str

    def render(self) -> str:
        return f"{self.rel_path}:{self.line_no}: [{self.rule}] {self.detail}"


def blank_test_modules(code: str) -> str:
    """Blank `#[cfg(test)] mod … { … }` bodies; tests build SSO values on purpose."""
    out = code
    cursor = 0
    while match := TEST_MOD_RE.search(out, cursor):
        opening = match.end() - 1
        closing = matching_brace(out, opening)
        if closing is None:
            break
        body = out[match.start() : closing + 1]
        out = out[: match.start()] + re.sub(r"[^\n]", " ", body) + out[closing + 1 :]
        cursor = closing + 1
    return out


def functions(code: str):
    """Yield (name, start, body) for every top-level-or-nested fn with a body."""
    cursor = 0
    while match := FUNCTION_RE.search(code, cursor):
        opening = code.find("{", match.end())
        semicolon = code.find(";", match.end())
        if opening < 0 or (0 <= semicolon < opening):
            cursor = match.end()
            continue
        closing = matching_brace(code, opening)
        if closing is None:
            break
        prefix = code[max(0, match.start() - 200) : match.start()]
        is_test = bool(TEST_FN_RE.search(prefix.split("}")[-1]))
        yield match.group(1), match.start(), code[match.start() : closing + 1], is_test
        cursor = closing + 1


def scan_mask_casts(crate: str, rel_path: str, text: str) -> list[Finding]:
    code = blank_test_modules(mask_non_code(text))
    findings: list[Finding] = []
    for name, start, body, is_test in functions(code):
        if is_test or SSO_MARKER_RE.search(body):
            continue
        for match in MASK_CAST_RE.finditer(body):
            line = code.count("\n", 0, start + match.start()) + 1
            findings.append(
                Finding(crate, rel_path, line, "mask-cast", f"fn {name}: masked bits cast to StringHeader")
            )
        heap_test = HEAP_TAG_RE.search(body)
        if heap_test and "StringHeader" in body or heap_test and "as_string_ptr" in body:
            line = code.count("\n", 0, start + heap_test.start()) + 1
            findings.append(
                Finding(crate, rel_path, line, "heap-tag-only", f"fn {name}: heap-only string test")
            )
    return findings


def string_param_table(root: Path) -> dict[str, set[int]]:
    """`extern "C" fn` name -> indexes of its `*const/*mut StringHeader` params."""
    table: dict[str, set[int]] = defaultdict(set)
    for crate_dir in crate_dirs(root):
        if crate_dir.name in ("perry-codegen", "perry-hir"):
            continue
        for path in sorted((crate_dir / "src").rglob("*.rs")) if (crate_dir / "src").is_dir() else []:
            text = path.read_text(encoding="utf-8")
            if "StringHeader" not in text:
                continue
            code = mask_non_code_keep_strings(text)
            for match in EXTERN_FN_RE.finditer(code):
                params = [p.strip() for p in re.split(r",(?![^<]*>)", match.group(2)) if p.strip()]
                body = code[match.end() : match.end() + 3000]
                for idx, param in enumerate(params):
                    if ":" not in param:
                        continue
                    pname, ptype = (part.strip() for part in param.split(":", 1))
                    pname = pname.replace("mut ", "").strip()
                    if "StringHeader" in ptype:
                        table[match.group(1)].add(idx)
                    elif ptype in ("i64", "u64", "usize") and re.search(
                        r"\b" + re.escape(pname) + r"\s+as\s+(?:usize\s+as\s+)?\*\s*(?:const|mut)\s+"
                        r"(?:[A-Za-z_][A-Za-z0-9_]*::)*StringHeader\b|"
                        r"string_from_header_i64\s*\(\s*" + re.escape(pname) + r"\s*\)",
                        body,
                    ):
                        # An `i64` parameter the body reads as a StringHeader.
                        table[match.group(1)].add(idx)
    return table


def scan_codegen(crate: str, rel_path: str, text: str, table: dict[str, set[int]]) -> list[Finding]:
    code = blank_test_modules(mask_non_code_keep_strings(text))
    findings: list[Finding] = []
    for name, start, body, is_test in functions(code):
        if is_test or "unbox_to_i64" not in body:
            continue
        for call in CALL_RE.finditer(body):
            callee = call.group(1)
            if callee not in table:
                continue
            args = re.split(r"\)\s*,\s*\(", call.group(2))
            for idx, arg in enumerate(args):
                if idx not in table[callee]:
                    continue
                refs = re.findall(r"&\s*([A-Za-z_][A-Za-z0-9_]*)", arg)
                if not refs:
                    continue
                operand = unboxed_operand(body[: call.start()], refs[-1])
                if operand is None:
                    continue
                line = code.count("\n", 0, start + call.start()) + 1
                findings.append(
                    Finding(
                        crate,
                        rel_path,
                        line,
                        "codegen-str-arg",
                        f"fn {name}: unbox_to_i64({operand}) -> {callee} arg {idx}",
                    )
                )
    return findings


def unboxed_operand(before: str, var: str) -> str | None:
    """The operand `var` was masked from, if its nearest binding is a bare
    `unbox_to_i64` of a non-literal value; otherwise None."""
    binding = None
    for match in re.finditer(r"let\s+(?:mut\s+)?" + re.escape(var) + r"\b[^=;]*=", before):
        binding = match
    if binding is None:
        return None
    unbox = UNBOX_LET_RE.match(before, binding.start())
    if unbox is None or unbox.group(1) != var:
        return None
    operand = unbox.group(2)
    if re.search(LITERAL_LOAD_RE_TMPL.format(name=re.escape(operand)), before):
        return None
    return operand


def mask_non_code_keep_strings(text: str) -> str:
    """Blank comments only: codegen names its callees with string literals."""
    out = re.sub(r"//[^\n]*", lambda m: " " * len(m.group(0)), text)
    return re.sub(r"/\*.*?\*/", lambda m: re.sub(r"[^\n]", " ", m.group(0)), out, flags=re.S)


def crate_dirs(root: Path = REPO_ROOT) -> list[Path]:
    crates = root / "crates"
    return sorted(path for path in crates.iterdir() if (path / "Cargo.toml").is_file())


def is_test_path(rel: Path) -> bool:
    name = rel.name
    return (
        "tests" in rel.parts
        or name == "tests.rs"
        or name.endswith("_tests.rs")
        or name.startswith("test_")
    )


def collect_inventory(root: Path = REPO_ROOT) -> tuple[list[Finding], int]:
    findings: list[Finding] = []
    files_scanned = 0
    table = string_param_table(root)
    for crate_dir in crate_dirs(root):
        crate = crate_dir.name
        for path in sorted(crate_dir.rglob("*.rs")):
            rel = path.relative_to(root)
            # Filter on the path RELATIVE to the repo root -- see the same
            # comment in string_payload_access_inventory.py (agents run under a
            # dot-prefixed `.claude/worktrees/` directory).
            if any(part.startswith(".") or part == "target" for part in rel.parts):
                continue
            if is_test_path(rel):
                continue
            files_scanned += 1
            text = path.read_text(encoding="utf-8")
            if crate == "perry-codegen":
                if "unbox_to_i64" in text:
                    findings.extend(scan_codegen(crate, rel.as_posix(), text, table))
            elif crate != "perry-hir" and "StringHeader" in text:
                findings.extend(scan_mask_casts(crate, rel.as_posix(), text))
    return findings, files_scanned


def counts_for(findings: list[Finding]) -> Counter[tuple[str, str]]:
    return Counter((finding.rule, finding.crate) for finding in findings)


def load_baseline(path: Path) -> dict[tuple[str, str], int]:
    baseline: dict[tuple[str, str], int] = {}
    if not path.is_file():
        return baseline
    errors: list[str] = []
    for line_no, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = [part.strip() for part in line.split("|", 2)]
        if len(parts) != 3 or parts[0] not in RULES or not parts[2].isdigit():
            errors.append(f"{path.name}:{line_no}: expected 'rule | crate | count', got: {raw}")
            continue
        key = (parts[0], parts[1])
        if key in baseline:
            errors.append(f"{path.name}:{line_no}: duplicate entry for {key}")
            continue
        baseline[key] = int(parts[2])
    if errors:
        print("\n".join(errors), file=sys.stderr)
        raise SystemExit(2)
    return baseline


def compare_counts(actual, baseline):
    regressions = []
    stale = []
    for rule, crate in sorted(set(actual) | set(baseline)):
        found = actual[(rule, crate)]
        allowed = baseline.get((rule, crate), 0)
        if found > allowed:
            regressions.append((rule, crate, allowed, found))
        elif found < allowed:
            stale.append((rule, crate, allowed, found))
    return regressions, stale


def write_baseline(path: Path, actual: Counter[tuple[str, str]]) -> None:
    lines = [
        "# SSO string unboxing baseline (#11519): see scripts/sso_unbox_inventory.py.",
        "# Format: rule | crate | count",
        "# Regenerate: python3 scripts/sso_unbox_inventory.py --write-baseline",
        "# Counts may only decrease; a new crate starts at zero.",
        "",
    ]
    for rule, crate in sorted(actual):
        if actual[(rule, crate)]:
            lines.append(f"{rule} | {crate} | {actual[(rule, crate)]}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


PLANTED_RUNTIME = r'''
pub extern "C" fn js_planted_date_parse(value: f64) -> f64 {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFF {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        return unsafe { (*ptr).byte_len as f64 };
    }
    f64::NAN
}

fn planted_value_reader(v: JSValue) -> usize {
    if v.is_string() {
        return unsafe { (*v.as_string_ptr()).byte_len as usize };
    }
    0
}

fn planted_unguarded(bits: u64) -> usize {
    let p = (bits & POINTER_MASK) as usize as *mut StringHeader;
    p as usize
}
'''

CLEAN_RUNTIME = r'''
pub extern "C" fn js_clean_date_parse(value: f64) -> f64 {
    crate::string::with_string_value_bytes(value, |b| b.len() as f64).unwrap_or(f64::NAN)
}

fn handles_both(bits: u64) -> usize {
    if JSValue::from_bits(bits).is_short_string() {
        return 0;
    }
    (bits & POINTER_MASK) as *const StringHeader as usize
}

// (bits & POINTER_MASK) as *const StringHeader in a comment is not code.
const DOC: &str = "(bits & POINTER_MASK) as *const StringHeader";

#[cfg(test)]
mod tests {
    fn planted_in_test(bits: u64) -> usize {
        (bits & POINTER_MASK) as *const StringHeader as usize
    }
}
'''

PLANTED_EXTERN = r'''
#[no_mangle]
pub extern "C" fn js_planted_native(handle: i64, name: *const StringHeader, n: f64) -> f64 { 0.0 }
'''

PLANTED_CODEGEN = r'''
fn lower_planted(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    let s_box = lower_expr(ctx, e)?;
    let blk = ctx.block();
    let s_handle = unbox_to_i64(blk, &s_box);
    Ok(blk.call(DOUBLE, "js_planted_native", &[(I64, &h), (I64, &s_handle), (DOUBLE, &n)]))
}

fn lower_literal_key(ctx: &mut FnCtx<'_>) -> Result<String> {
    let blk = ctx.block();
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_handle = unbox_to_i64(blk, &key_box);
    Ok(blk.call(DOUBLE, "js_planted_native", &[(I64, &h), (I64, &key_handle), (DOUBLE, &n)]))
}

fn lower_object_arg(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    let o_box = lower_expr(ctx, e)?;
    let blk = ctx.block();
    let o_handle = unbox_to_i64(blk, &o_box);
    Ok(blk.call(DOUBLE, "js_planted_native", &[(I64, &o_handle), (I64, &s), (DOUBLE, &n)]))
}

fn lower_fixed(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    let s_box = lower_expr(ctx, e)?;
    let blk = ctx.block();
    let s_handle = unbox_ffi_str_arg(blk, &s_box);
    Ok(blk.call(DOUBLE, "js_planted_native", &[(I64, &h), (I64, &s_handle), (DOUBLE, &n)]))
}
'''


def run_self_tests() -> int:
    failures: list[str] = []

    def expect(condition: bool, message: str) -> None:
        if not condition:
            failures.append(message)

    planted = scan_mask_casts("synthetic", "crates/synthetic/src/lib.rs", PLANTED_RUNTIME)
    expect(
        sum(f.rule == "mask-cast" for f in planted) == 2,
        f"planted heap-only and unguarded mask casts: expected 2 mask-cast findings, got {planted}",
    )
    expect(
        sum(f.rule == "heap-tag-only" for f in planted) == 2,
        f"planted `== 0x7FFF` and `.is_string()` readers: expected 2 heap-tag-only findings, got {planted}",
    )
    clean = scan_mask_casts("synthetic", "crates/synthetic/src/lib.rs", CLEAN_RUNTIME)
    expect(not clean, f"SSO-aware code, comments, strings or cfg(test) produced findings: {clean}")

    with tempfile.TemporaryDirectory() as temp_dir:
        root = Path(temp_dir)
        for crate, rel, text in (
            ("perry-runtime", "src/lib.rs", PLANTED_RUNTIME + PLANTED_EXTERN),
            ("perry-codegen", "src/expr/planted.rs", PLANTED_CODEGEN),
        ):
            crate_dir = root / "crates" / crate
            (crate_dir / Path(rel).parent).mkdir(parents=True, exist_ok=True)
            (crate_dir / "Cargo.toml").write_text(
                f'[package]\nname = "{crate}"\nversion = "0.0.0"\n', encoding="utf-8"
            )
            (crate_dir / rel).write_text(text, encoding="utf-8")
        table = string_param_table(root)
        expect(table.get("js_planted_native") == {1}, f"string-param table wrong: {dict(table)}")
        findings, scanned = collect_inventory(root)
        expect(scanned == 2, f"expected 2 files scanned, got {scanned}")
        counts = counts_for(findings)
        expect(
            counts[("mask-cast", "perry-runtime")] == 2
            and counts[("heap-tag-only", "perry-runtime")] == 2,
            f"end-to-end runtime counts wrong: {dict(counts)}",
        )
        # Exactly the user-value operand: not the literal key, not the object
        # operand in a non-string position, not the already-fixed call.
        expect(
            counts[("codegen-str-arg", "perry-codegen")] == 1,
            f"end-to-end codegen-str-arg count wrong: {[f.render() for f in findings]}",
        )
        regressions, stale = compare_counts(counts, {})
        expect(bool(regressions) and not stale, "a zero baseline did not reject the planted offenders")
        regressions, stale = compare_counts(counts, dict(counts))
        expect(not regressions and not stale, "a matching baseline was not accepted")
        lowered = dict(counts)
        lowered[("mask-cast", "perry-runtime")] += 1
        regressions, stale = compare_counts(counts, lowered)
        expect(not regressions and bool(stale), "a removed offender did not require a repin")

    if failures:
        for failure in failures:
            print(f"self-test failure: {failure}", file=sys.stderr)
        return 1
    print("sso-unbox inventory self-tests passed")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--list", action="store_true", help="print every finding")
    parser.add_argument("--write-baseline", action="store_true")
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_tests()

    findings, files_scanned = collect_inventory()
    if files_scanned == 0:
        print(
            "sso-unbox inventory: SCANNED NO FILES -- this is a broken scan, not a "
            "converted tree. Do NOT run --write-baseline.",
            file=sys.stderr,
        )
        return 1
    actual = counts_for(findings)
    if args.write_baseline:
        write_baseline(args.baseline, actual)
        print(f"wrote {args.baseline.relative_to(REPO_ROOT)}")
        return 0
    if args.list:
        for finding in findings:
            print(finding.render())

    baseline = load_baseline(args.baseline)
    regressions, stale = compare_counts(actual, baseline)
    if regressions:
        print("SSO string-unboxing ratchet increased:", file=sys.stderr)
        for rule, crate, allowed, found in regressions:
            print(f"  {rule} | {crate}: baseline {allowed}, found {found}", file=sys.stderr)
            for finding in findings:
                if finding.rule == rule and finding.crate == crate:
                    print(f"    {finding.render()}", file=sys.stderr)
        print(
            "A short string (<= 5 bytes) is stored inline under SHORT_STRING_TAG and has "
            "no StringHeader; see this script's docstring for the SSO-aware accessors.",
            file=sys.stderr,
        )
    if stale:
        print("SSO string-unboxing baseline is stale; record the progress:", file=sys.stderr)
        for rule, crate, allowed, found in stale:
            print(f"  {rule} | {crate}: baseline {allowed}, found {found}", file=sys.stderr)
    if regressions or stale:
        print("Run: python3 scripts/sso_unbox_inventory.py --write-baseline", file=sys.stderr)
        return 1

    totals = Counter()
    for (rule, _crate), count in actual.items():
        totals[rule] += count
    print(
        f"sso-unbox inventory: {files_scanned} files; {totals['mask-cast']} mask casts, "
        f"{totals['heap-tag-only']} heap-only string readers and "
        f"{totals['codegen-str-arg']} codegen string args held by the ratchet"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
