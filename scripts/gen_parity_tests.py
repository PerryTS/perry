#!/usr/bin/env python3
"""Generate test_parity_<module>.ts skeletons from docs/runtime-parity.md.

Each generated file is a parity test that `run_parity_tests.sh` picks up
automatically (any *.ts in test-files/ is run by both Perry and
`node --experimental-strip-types`; the outputs are diffed byte-for-byte).

Workflow:

    scripts/gen_parity_tests.py                    # all modules
    scripts/gen_parity_tests.py --module path      # one module
    scripts/gen_parity_tests.py --dry-run          # print plan
    scripts/gen_parity_tests.py --force            # overwrite existing

Trivial shapes (zero-arg methods, bare constants/properties) auto-emit
as `console.log("api:", expr)` lines. Anything else becomes a TODO
comment for a human. The skiplist (scripts/parity-skiplist.toml) marks
modules and APIs we explicitly will not pursue parity for.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

try:
    import tomllib
except ImportError:
    import tomli as tomllib  # type: ignore


REPO_ROOT = Path(__file__).resolve().parent.parent
PARITY_DOC = REPO_ROOT / "docs" / "runtime-parity.md"
SKIPLIST = REPO_ROOT / "scripts" / "parity-skiplist.toml"
OUTPUT_DIR = REPO_ROOT / "test-files"

MODULE_HEADER = re.compile(r"^### node:([\w/]+)(?: \(.*\))?$")
SECTION_HEADER = re.compile(r"^### (.+)$")
TABLE_ROW = re.compile(r"^\|\s*`([^`]+)`\s*\|\s*([^|]*?)\s*\|\s*([^|]*?)\s*\|\s*([^|]*?)\s*\|\s*$")
HEADER_BREAK = re.compile(r"^\|[\s:|-]+\|\s*$")

NODE_NOT_PRESENT = {"✗", "✗ (Bun-only)"}


@dataclass
class ApiRow:
    name: str
    node: str
    bun: str
    notes: str
    section: str


@dataclass
class ModuleSpec:
    name: str
    apis: list[ApiRow] = field(default_factory=list)

    @property
    def filename_safe(self) -> str:
        return self.name.replace("/", "_")


def parse_parity_doc(path: Path) -> list[ModuleSpec]:
    modules: list[ModuleSpec] = []
    current: ModuleSpec | None = None
    current_section = ""
    in_table = False

    with path.open() as f:
        for line in f:
            line = line.rstrip("\n")

            m = MODULE_HEADER.match(line)
            if m:
                if current is not None:
                    modules.append(current)
                current = ModuleSpec(name=m.group(1))
                current_section = ""
                in_table = False
                continue

            if SECTION_HEADER.match(line) and not MODULE_HEADER.match(line):
                if current is not None:
                    modules.append(current)
                    current = None
                continue

            if current is None:
                continue

            if line.startswith("#### "):
                current_section = line[5:].strip()
                in_table = False
                continue

            if HEADER_BREAK.match(line):
                in_table = True
                continue

            if in_table:
                row = TABLE_ROW.match(line)
                if row:
                    current.apis.append(ApiRow(
                        name=row.group(1).strip(),
                        node=row.group(2).strip(),
                        bun=row.group(3).strip(),
                        notes=row.group(4).strip(),
                        section=current_section,
                    ))
                else:
                    in_table = False

    if current is not None:
        modules.append(current)

    return modules


def load_skiplist(path: Path) -> tuple[dict[str, str], dict[str, str]]:
    if not path.exists():
        return {}, {}
    with path.open("rb") as f:
        data = tomllib.load(f)
    return data.get("skip-modules", {}), data.get("skip-apis", {})


RE_PROPERTY = re.compile(r"^[\w.]+$")
RE_ZERO_ARG_CALL = re.compile(r"^([\w.]+)\(\)$")
RE_CLASS_DECL = re.compile(r"^class\s+[\w.]+")
RE_NEW_EXPR = re.compile(r"^new\s+[\w.]+")
RE_PROTOTYPE = re.compile(r"^\w+\.prototype\.")


def section_is_class_scoped(section: str) -> bool:
    """A subsection like "Class: URL" means every row inside is a class
    member — even when the parity doc uses a lowercase instance variable
    that collides with the module's import binding (e.g. `url.hash` inside
    the URL section refers to a URL instance, not the namespace import)."""
    s = section.lower()
    return s.startswith("class:") or s.startswith("class ")


def emit_line(api: ApiRow, module: str) -> str | None:
    name = api.name

    if section_is_class_scoped(api.section):
        return f'// TODO(class-member): {name}'

    if name.startswith("."):
        return f'// TODO(method): {name}'

    if RE_CLASS_DECL.match(name) or RE_NEW_EXPR.match(name):
        return f'// TODO(class): {name}'

    if RE_PROTOTYPE.match(name):
        return f'// TODO(proto): {name}'

    leading = name.split(".", 1)[0]
    import_binding = module.replace("/", "_")
    if leading != import_binding:
        if RE_PROPERTY.match(name) or RE_ZERO_ARG_CALL.match(name):
            return f'// TODO(scoped): {name}'
        return f'// TODO(call): {name}'

    if RE_PROPERTY.match(name):
        return f'console.log("{name}:", {name});'

    m = RE_ZERO_ARG_CALL.match(name)
    if m:
        call = m.group(1)
        return f'console.log("{call}():", {call}());'

    return f'// TODO(call): {name}'


def needs_import(module: str) -> str | None:
    if module in ("buffer",):
        return f'import {{ Buffer }} from "node:buffer";'
    return f'import * as {module.replace("/", "_")} from "node:{module}";'


# Modules whose parity tests must run under PERRY_DETERMINISTIC_NET=1 so the
# in-process loopback answers stay reproducible across machines (#4911). The
# real network stack is exercised separately; byte-for-byte parity needs a
# deterministic resolver/socket.
DETERMINISTIC_NET_MODULES = {"dns", "dns/promises", "dgram"}


def emit_file(spec: ModuleSpec, skip_apis: dict[str, str]) -> str:
    lines: list[str] = []
    if spec.name in DETERMINISTIC_NET_MODULES:
        lines.append("// parity-env: PERRY_DETERMINISTIC_NET=1")
    lines.append(f"// Auto-generated by scripts/gen_parity_tests.py from docs/runtime-parity.md.")
    lines.append(f"// Module: node:{spec.name}")
    lines.append(f"// This file is a byte-for-byte parity test against `node --experimental-strip-types`.")
    lines.append(f"// Edit the TODO lines below to exercise each API; rerun")
    lines.append(f"// `./run_parity_tests.sh --filter test_parity_{spec.filename_safe}` to verify.")
    lines.append("")

    imp = needs_import(spec.name)
    if imp:
        lines.append(imp)
        lines.append("")

    current_section = ""
    emitted_count = 0
    todo_count = 0
    skipped_count = 0
    seen: set[str] = set()

    for api in spec.apis:
        if api.name in seen:
            continue
        seen.add(api.name)

        if api.node in NODE_NOT_PRESENT:
            continue

        if api.section != current_section:
            current_section = api.section
            lines.append("")
            lines.append(f"// ── {api.section} ──")

        skip_reason = skip_apis.get(api.name)
        if skip_reason:
            lines.append(f'// SKIP: {api.name} — {skip_reason}')
            skipped_count += 1
            continue

        line = emit_line(api, spec.name)
        if line is None:
            continue

        if line.startswith("// TODO"):
            todo_count += 1
        else:
            emitted_count += 1

        lines.append(line)

    lines.append("")
    lines.append(f"// Coverage: {emitted_count} auto-emitted, {todo_count} TODO, {skipped_count} skip-listed.")
    lines.append("")

    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--module", help="Generate only the given module name (e.g. 'path').")
    ap.add_argument("--dry-run", action="store_true", help="Print the generation plan without writing files.")
    ap.add_argument("--force", action="store_true", help="Overwrite existing generated files even if a human edited them.")
    ap.add_argument("--parity-doc", type=Path, default=PARITY_DOC)
    ap.add_argument("--skiplist", type=Path, default=SKIPLIST)
    ap.add_argument("--output-dir", type=Path, default=OUTPUT_DIR)
    args = ap.parse_args()

    if not args.parity_doc.exists():
        sys.exit(f"error: parity doc not found: {args.parity_doc}")

    modules = parse_parity_doc(args.parity_doc)
    skip_modules, skip_apis = load_skiplist(args.skiplist)

    targets = modules
    if args.module:
        targets = [m for m in modules if m.name == args.module]
        if not targets:
            available = ", ".join(sorted(m.name for m in modules))
            sys.exit(f"error: module '{args.module}' not found. Available: {available}")

    args.output_dir.mkdir(parents=True, exist_ok=True)

    written = 0
    skipped = 0
    for spec in targets:
        if spec.name in skip_modules:
            print(f"skip-module: {spec.name} ({skip_modules[spec.name]})")
            skipped += 1
            continue

        out_path = args.output_dir / f"test_parity_{spec.filename_safe}.ts"
        contents = emit_file(spec, skip_apis)

        if args.dry_run:
            print(f"would write: {out_path} ({len(spec.apis)} rows)")
            continue

        if out_path.exists() and not args.force:
            existing = out_path.read_text()
            first_line = existing.splitlines()[0] if existing else ""
            if "// Auto-generated by scripts/gen_parity_tests.py" not in first_line:
                print(f"preserving (hand-edited): {out_path}")
                skipped += 1
                continue

        out_path.write_text(contents)
        print(f"wrote: {out_path}")
        written += 1

    if not args.dry_run:
        print(f"\ndone — {written} files written, {skipped} skipped.")
    else:
        print(f"\ndry-run — {len(targets)} modules would be generated.")


if __name__ == "__main__":
    main()
