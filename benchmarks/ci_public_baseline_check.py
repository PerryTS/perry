#!/usr/bin/env python3
"""CI freshness gate for the public Node/Bun benchmark artifact.

Thin wrapper over :mod:`public_baseline` that keeps the parts that still matter —
artifact freshness/integrity (age, schema, source + harness fingerprints) and
``benchmarks/suite/results/RESULTS.md`` drift — while treating the *embedded
README table as optional*: README drift is enforced only when the generated
``<!-- public-node-bun:start/end -->`` markers are present.

Rationale: #6736 rewrote ``README.md`` as a concise marketing landing page with a
different, hand-curated performance section and intentionally dropped the
auto-generated public-node-bun block. ``public_baseline.py check`` still *requires*
that block, so it fails on every PR ("README generated markers are missing").

This policy lives in a SEPARATE module on purpose: the checker and assembler are
plumbing rather than measurement inputs. ``HARNESS_PATHS`` fingerprints the
declarative measurement config and correctness oracle instead, while this thin
wrapper reuses the same validation functions and cannot drift into a second
implementation of the freshness policy.
"""

from __future__ import annotations

import sys

import public_baseline as pb

MAX_AGE_DAYS = 45


def main() -> int:
    try:
        artifact = pb._load(pb.DEFAULT_ARTIFACT)
        # Freshness + integrity (age, schema, source/harness fingerprints).
        pb.validate_public(artifact, MAX_AGE_DAYS)
        # Generated docs table must still track the artifact.
        if pb.suite_results(artifact) != pb.SUITE_RESULTS.read_text(encoding="utf-8"):
            raise pb.ArtifactError("suite RESULTS.md has drifted from the public artifact")
        # README table is optional; enforce drift only when the markers exist.
        readme = pb.README.read_text(encoding="utf-8")
        if pb.README_START in readme and pb.README_END in readme:
            if pb._replace_block(readme, pb.readme_block(artifact)) != readme:
                raise pb.ArtifactError(
                    "README Node/Bun table has drifted from the public artifact"
                )
    except (pb.ArtifactError, KeyError, OSError) as exc:
        print(f"public baseline error: {exc}", file=sys.stderr)
        return 2
    print("public baseline freshness OK (embedded README table optional)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
