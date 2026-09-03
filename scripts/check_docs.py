#!/usr/bin/env python3
"""Check mdBook navigation, local links/anchors, and README release links.

This intentionally avoids network access. The scheduled docs workflow runs a
separate external-link checker so transient remote failures do not block every
documentation pull request.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path
from urllib.parse import unquote, urlparse


ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
SRC = DOCS / "src"
SUMMARY = SRC / "SUMMARY.md"
README = ROOT / "README.md"

FENCE_RE = re.compile(r"^\s*(```+|~~~+)")
INLINE_LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
REFERENCE_LINK_RE = re.compile(r"^\s*\[[^\]]+\]:\s*(\S+)", re.MULTILINE)
INCLUDE_RE = re.compile(r"\{\{#(?:rustdoc_)?include\s+([^}\s]+)")
HEADING_RE = re.compile(r"^\s{0,3}(#{1,6})\s+(.+?)\s*#*\s*$")
HTML_ID_RE = re.compile(r"\bid=[\"']([^\"']+)[\"']")
RELEASE_TAG_RE = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")

FORBIDDEN_TEXT = {
    "https://github.com/skelpo/perry": "https://github.com/PerryTS/perry",
    "https://github.com/perry-ts/perry": "https://github.com/PerryTS/perry",
    "https://github.com/PerryTS/mango": "https://github.com/MangoQuery/app",
    "@perry/iroh": "@perryts/iroh",
    "@perry/dotenv": "@perryts/dotenv",
    "perryts/mysql2-bindings": "@perryts/tursodb",
}


def latest_release_tag() -> str | None:
    """Return the highest stable vX.Y.Z tag available in this checkout."""
    configured = os.environ.get("PERRY_DOCS_RELEASE_TAG")
    if configured is not None:
        return configured if RELEASE_TAG_RE.fullmatch(configured) else None

    try:
        result = subprocess.run(
            ["git", "tag", "--list", "v*"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None

    releases: list[tuple[tuple[int, int, int], str]] = []
    for tag in result.stdout.splitlines():
        match = RELEASE_TAG_RE.fullmatch(tag)
        if match:
            releases.append((tuple(map(int, match.groups())), tag))
    return max(releases)[1] if releases else None


def published_docs_at(tag: str) -> set[str] | None:
    """List mdBook chapters published by a release tag's SUMMARY."""
    try:
        result = subprocess.run(
            ["git", "show", f"{tag}:docs/src/SUMMARY.md"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None

    published: set[str] = set()
    for raw in local_destinations(result.stdout):
        rel, _ = split_destination(raw)
        if rel and not rel.startswith(("http://", "https://", "mailto:")):
            published.add((Path("docs/src") / rel).as_posix())
    return published


def public_docs_path(destination: str) -> str | None:
    """Map either public docs hostname to its mdBook-relative URL path."""
    parsed = urlparse(destination)
    path = unquote(parsed.path)
    if parsed.hostname == "perryts.github.io":
        if path.rstrip("/") == "/perry":
            return ""
        prefix = "/perry/"
        if not path.startswith(prefix):
            return None
        return path[len(prefix) :]
    if parsed.hostname == "docs.perryts.com":
        return path.lstrip("/")
    return None


def readme_release_link_errors(
    text: str, released_docs: set[str], release_tag: str
) -> list[str]:
    """Reject public README pages that the deployed release cannot contain."""
    errors: list[str] = []
    for raw in local_destinations(text):
        destination, _ = split_destination(raw)
        web_path = public_docs_path(destination)
        if web_path is None or web_path in {"", "index.html"}:
            continue
        if not web_path.endswith(".html"):
            errors.append(
                f"README.md: public docs link is not an mdBook page: {destination}"
            )
            continue
        source = f"docs/src/{web_path.removesuffix('.html')}.md"
        if source not in released_docs:
            errors.append(
                f"README.md: public docs link is absent from {release_tag}: "
                f"{destination}; link to {source} until it ships in a release"
            )
    return errors


def self_test() -> int:
    released = {"docs/src/guide/released.md"}
    fixture = """
[home](https://perryts.github.io/perry/)
[released](https://perryts.github.io/perry/guide/released.html#section)
[canonical](https://docs.perryts.com/guide/released.html)
[external](https://example.com/guide/unreleased.html)
"""
    if readme_release_link_errors(fixture, released, "v1.2.3"):
        print("check_docs --self-test FAILED: accepted fixture was rejected")
        return 1

    planted = (
        fixture
        + "\n[unreleased](https://perryts.github.io/perry/guide/unreleased.html)\n"
    )
    errors = readme_release_link_errors(planted, released, "v1.2.3")
    if len(errors) != 1 or "docs/src/guide/unreleased.md" not in errors[0]:
        print("check_docs --self-test FAILED: planted unreleased page was not caught")
        return 1

    print("check_docs --self-test passed: released aliases pass and an unreleased page fails")
    return 0


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

    readme_text = README.read_text(encoding="utf-8")
    release_tag = latest_release_tag()
    if release_tag is None:
        errors.append(
            "README.md: cannot find a stable release tag; fetch tags before checking docs"
        )
    else:
        released_docs = published_docs_at(release_tag)
        if released_docs is None:
            errors.append(f"README.md: cannot inspect documentation at {release_tag}")
        else:
            errors.extend(
                readme_release_link_errors(readme_text, released_docs, release_tag)
            )

    for raw in local_destinations(readme_text):
        rel, anchor = split_destination(raw)
        if not rel or rel.startswith(("http://", "https://", "mailto:")):
            continue
        target = (README.parent / rel).resolve()
        if not target.is_file():
            errors.append(f"README.md: local link target does not exist: {raw}")
            continue
        if anchor and target.suffix.lower() == ".md":
            anchors = anchors_for(target)
            if anchor not in anchors:
                errors.append(
                    f"README.md: anchor #{anchor} not found in {target.relative_to(ROOT)}"
                )

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
        f"all listed with valid local links, anchors, and include targets; "
        f"README public pages present in {release_tag}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(self_test() if sys.argv[1:] == ["--self-test"] else main())
