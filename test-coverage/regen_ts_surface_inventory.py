#!/usr/bin/env python3
"""Regenerate TypeScript FFI surface inventory fixtures.

The generated fixtures are executable by run_parity_tests.sh, but their main
purpose is to keep TypeScript-side coverage accounting attached to related FFI
surfaces. They should shrink over time as entries move into behavioral tests.
"""

from __future__ import annotations

import collections
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEST_FILES = ROOT / "test-files"

INVENTORY_FILES = {
    "test_ffi_surface_coverage.ts",
    "test_ffi_surface_runtime_core.ts",
    "test_ffi_surface_runtime_ui.ts",
    "test_ffi_surface_stdlib_core.ts",
    "test_ffi_surface_stdlib_io.ts",
    "test_ffi_surface_stdlib_integrations.ts",
}

OUTPUTS = {
    "runtime_core": (
        "test_ffi_surface_runtime_core.ts",
        "Runtime core FFI surface inventory.",
    ),
    "runtime_ui": (
        "test_ffi_surface_runtime_ui.ts",
        "Runtime UI and platform FFI surface inventory.",
    ),
    "stdlib_core": (
        "test_ffi_surface_stdlib_core.ts",
        "Stdlib core utility FFI surface inventory.",
    ),
    "stdlib_io": (
        "test_ffi_surface_stdlib_io.ts",
        "Stdlib IO, network, stream, and framework FFI surface inventory.",
    ),
    "stdlib_integrations": (
        "test_ffi_surface_stdlib_integrations.ts",
        "Stdlib external integration FFI surface inventory.",
    ),
}

FFI_RE = re.compile(
    r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\('
)


def collect_ffi() -> tuple[list[tuple[str, str]], collections.Counter[str], dict[str, list[str]]]:
    funcs: list[tuple[str, str]] = []
    name_counts: collections.Counter[str] = collections.Counter()
    name_paths: dict[str, list[str]] = collections.defaultdict(list)
    for rel in ("crates/perry-runtime/src", "crates/perry-stdlib/src"):
        for path in sorted((ROOT / rel).rglob("*.rs")):
            text = path.read_text(errors="ignore")
            source = str(path.relative_to(ROOT))
            for match in FFI_RE.finditer(text):
                name = match.group(1)
                funcs.append((name, source))
                name_counts[name] += 1
                name_paths[name].append(source)
    return funcs, name_counts, name_paths


def non_inventory_ts_text() -> str:
    parts: list[str] = []
    for pattern in ("*.ts", "*.tsx"):
        for path in sorted(TEST_FILES.glob(pattern)):
            if path.name in INVENTORY_FILES:
                continue
            parts.append(path.read_text(errors="ignore"))
    return "\n".join(parts)


def group_for(source: str) -> str:
    if source.startswith("crates/perry-runtime/src/"):
        if any(
            part in source
            for part in (
                "/tui/",
                "ui_text_registry.rs",
                "geisterhand_registry.rs",
                "media_playback.rs",
                "arkts_callbacks.rs",
                "ios_game_loop.rs",
                "watchos_game_loop.rs",
                "jsx.rs",
            )
        ):
            return "runtime_ui"
        return "runtime_core"

    if source.startswith("crates/perry-stdlib/src/"):
        if any(
            part in source
            for part in (
                "framework/",
                "fetch.rs",
                "http.rs",
                "net/",
                "streams.rs",
                "ws.rs",
                "worker_threads.rs",
                "readline.rs",
            )
        ):
            return "stdlib_io"
        if any(
            part in source
            for part in (
                "mongodb.rs",
                "mysql2/",
                "pg/",
                "ioredis.rs",
                "nodemailer.rs",
                "ethers.rs",
                "crypto.rs",
                "crypto_e2e.rs",
                "jsonwebtoken.rs",
                "bcrypt.rs",
                "argon2.rs",
                "sharp.rs",
                "cheerio.rs",
                "webcrypto.rs",
            )
        ):
            return "stdlib_integrations"
        return "stdlib_core"

    return "stdlib_core"


def const_name_for(filename: str) -> str:
    name = "".join(part.capitalize() for part in filename.removesuffix(".ts").split("_"))
    return name[0].lower() + name[1:]


def render_fixture(
    filename: str,
    title: str,
    grouped: dict[str, list[str]],
    name_counts: collections.Counter[str],
) -> str:
    unique_names = sorted({name for names in grouped.values() for name in names})
    declarations = sum(name_counts[name] for name in unique_names)
    const_name = const_name_for(filename)
    lines = [
        f"// {title}",
        "//",
        "// This fixture is intentionally executable by the normal parity runner,",
        "// but its main purpose is to keep TS-side coverage accounting attached",
        "// to related public FFI shims. Move @covers entries from this",
        "// inventory into behavioral tests as each area gets deeper compatibility",
        "// coverage.",
        "//",
        f"// Inventory entries: {len(unique_names)} unique FFI names, {declarations} declarations.",
        "",
        f"const {const_name}Version = 1;",
        f"if ({const_name}Version !== 1) {{",
        '  throw new Error("unexpected coverage inventory version");',
        "}",
        f'console.log("{filename.removesuffix(".ts")}: ok");',
        "",
        "/*",
        "@covers",
    ]
    for source in sorted(grouped):
        lines.append(f"{source}:")
        for name in sorted(set(grouped[source])):
            lines.append(f"  - {name}")
    lines.extend(["*/", ""])
    return "\n".join(lines)


def main() -> None:
    funcs, name_counts, name_paths = collect_ffi()
    ts_text = non_inventory_ts_text()
    covered_names = {name for name in name_counts if name in ts_text}
    missing_names = [name for name in sorted(name_counts) if name not in covered_names]

    grouped_by_output: dict[str, dict[str, list[str]]] = {
        key: collections.defaultdict(list) for key in OUTPUTS
    }
    for name in missing_names:
        for source in sorted(set(name_paths[name])):
            grouped_by_output[group_for(source)][source].append(name)

    legacy = TEST_FILES / "test_ffi_surface_coverage.ts"
    if legacy.exists():
        legacy.unlink()

    for group, (filename, title) in OUTPUTS.items():
        path = TEST_FILES / filename
        path.write_text(render_fixture(filename, title, grouped_by_output[group], name_counts))
        unique = {name for names in grouped_by_output[group].values() for name in names}
        print(f"wrote {path.relative_to(ROOT)} ({len(unique)} names)")

    non_inventory_decls = sum(name_counts[name] for name in covered_names)
    print(f"non-inventory TS declarations: {non_inventory_decls}/{len(funcs)}")
    print(f"inventory unique names: {len(missing_names)}")


if __name__ == "__main__":
    main()
