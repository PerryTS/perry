#!/usr/bin/env python3
"""Check mdBook navigation, local links/anchors, and include targets.

This intentionally avoids network access. The scheduled docs workflow runs a
separate external-link checker so transient remote failures do not block every
documentation pull request.
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
SRC = DOCS / "src"
SUMMARY = SRC / "SUMMARY.md"

FENCE_RE = re.compile(r"^\s*(```+|~~~+)")
INLINE_LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
REFERENCE_LINK_RE = re.compile(r"^\s*\[[^\]]+\]:\s*(\S+)", re.MULTILINE)
INCLUDE_RE = re.compile(r"\{\{#(?:rustdoc_)?include\s+([^}\s]+)")
HEADING_RE = re.compile(r"^\s{0,3}(#{1,6})\s+(.+?)\s*#*\s*$")
HTML_ID_RE = re.compile(r"\bid=[\"']([^\"']+)[\"']")

FORBIDDEN_TEXT = {
    "https://github.com/skelpo/perry": "https://github.com/PerryTS/perry",
    "https://github.com/perry-ts/perry": "https://github.com/PerryTS/perry",
    "https://github.com/PerryTS/mango": "https://github.com/MangoQuery/app",
    "@perry/iroh": "@perryts/iroh",
    "@perry/dotenv": "@perryts/dotenv",
    "perryts/mysql2-bindings": "@perryts/tursodb",
}


def without_fenced_code(text: str) -> str:
    out: list[str] = []
    fence: str | None = None
    for line in text.splitlines():
        match = FENCE_RE.match(line)
        if match:
            marker = match.group(1)[0]
            if fence is None:
                fence = marker
            elif fence == marker:
                fence = None
            out.append("")
        elif fence is None:
            out.append(line)
        else:
            out.append("")
    return "\n".join(out)


def slugify_heading(raw: str) -> str:
    # mdBook/pulldown-cmark heading IDs are derived from rendered text. Remove
    # common inline markup before applying the lowercase/hyphen normalization.
    text = re.sub(r"<[^>]+>", "", raw)
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"[`*~]", "", text).lower()
    text = re.sub(r"\s+", "-", text.strip())
    text = "".join(ch for ch in text if ch.isalnum() or ch in "-_ ")
    return text.replace(" ", "-")


def anchors_for(path: Path) -> set[str]:
    text = without_fenced_code(path.read_text(encoding="utf-8"))
    anchors: set[str] = set(HTML_ID_RE.findall(text))
    seen: Counter[str] = Counter()
    for line in text.splitlines():
        match = HEADING_RE.match(line)
        if not match:
            continue
        base = slugify_heading(match.group(2))
        if not base:
            continue
        count = seen[base]
        seen[base] += 1
        anchors.add(base if count == 0 else f"{base}-{count}")
    return anchors


def split_destination(raw: str) -> tuple[str, str | None]:
    destination = raw.strip().strip("<>")
    # Drop an optional Markdown title after a whitespace separator.
    destination = re.split(r"\s+[\"']", destination, maxsplit=1)[0]
    path, sep, anchor = destination.partition("#")
    return unquote(path), unquote(anchor) if sep else None


def local_destinations(text: str) -> list[str]:
    clean = without_fenced_code(text)
    clean = re.sub(r"`+[^`]*`+", "", clean)
    return INLINE_LINK_RE.findall(clean) + REFERENCE_LINK_RE.findall(clean)


def include_file(raw: str) -> str:
    # mdBook regions/ranges are suffixes (`file.rs:region`, `file.rs:2:5`).
    # A Windows drive never appears in repository-relative include syntax.
    return raw.split(":", 1)[0]


def main() -> int:
    errors: list[str] = []
    markdown = sorted(SRC.rglob("*.md"))

    summary_text = SUMMARY.read_text(encoding="utf-8")
    listed: set[Path] = set()
    for raw in local_destinations(summary_text):
        rel, _ = split_destination(raw)
        if not rel or rel.startswith(("http://", "https://", "mailto:")):
            continue
        target = (SUMMARY.parent / rel).resolve()
        listed.add(target)
        if not target.is_file():
            errors.append(f"docs/src/SUMMARY.md: missing chapter target {rel}")

    for path in markdown:
        if path == SUMMARY:
            continue
        if path.resolve() not in listed:
            errors.append(f"{path.relative_to(ROOT)}: not listed in docs/src/SUMMARY.md")

    anchor_cache: dict[Path, set[str]] = {}
    for path in markdown:
        text = path.read_text(encoding="utf-8")

        for old, replacement in FORBIDDEN_TEXT.items():
            if old in text:
                errors.append(
                    f"{path.relative_to(ROOT)}: legacy text {old}; use {replacement}"
                )

        for raw in local_destinations(text):
            rel, anchor = split_destination(raw)
            if rel.startswith(("http://", "https://", "mailto:", "javascript:")):
                continue
            if rel:
                target = (path.parent / rel).resolve()
                if not target.is_file():
                    errors.append(
                        f"{path.relative_to(ROOT)}: local link target does not exist: {raw}"
                    )
                    continue
            else:
                target = path.resolve()

            if anchor and target.suffix.lower() == ".md":
                anchors = anchor_cache.setdefault(target, anchors_for(target))
                if anchor not in anchors:
                    errors.append(
                        f"{path.relative_to(ROOT)}: anchor #{anchor} not found in "
                        f"{target.relative_to(ROOT)}"
                    )

        for raw in INCLUDE_RE.findall(text):
            rel = include_file(raw)
            target = (path.parent / rel).resolve()
            if not target.is_file():
                errors.append(
                    f"{path.relative_to(ROOT)}: include target does not exist: {raw}"
                )

    # Translated link targets live in msgstr lines and can remain stale after
    # msgmerge marks a changed source entry fuzzy. Ignore obsolete (#~) entries,
    # but reject legacy URLs in every active translation.
    for path in sorted((DOCS / "po").glob("*.po")):
        for line_no, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            if line.startswith("#~"):
                continue
            for old, replacement in FORBIDDEN_TEXT.items():
                if old in line:
                    errors.append(
                        f"{path.relative_to(ROOT)}:{line_no}: legacy translated text "
                        f"{old}; use {replacement}"
                    )

    if errors:
        print("documentation checks failed:", file=sys.stderr)
        for error in sorted(set(errors)):
            print(f"  - {error}", file=sys.stderr)
        return 1

    print(
        f"documentation checks passed: {len(markdown) - 1} chapters, "
        "all listed with valid local links, anchors, and include targets"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
