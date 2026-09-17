#!/usr/bin/env python3
"""Gate: no two bindings may claim the same turnloop completion-sink slot.

A binding is a separately linked `staticlib` that registers a sink function for
its slot with `js_perry_net_register_sink`, and the runtime routes every
completion by that slot number (`perry-runtime/src/turnloop_net/sink.rs`). Two
bindings on one slot is a correctness bug: before the runtime learned to refuse
a duplicate, `register_sink` stored and returned true to BOTH, so each believed
it was registered while every completion went to whichever registered last —
which reads the token's low bits as one of its OWN connection ids.

That is not hypothetical. The P7 database lane and the P5 server lane numbered
from two different ledgers, and three pairs collided in shipped code:
`perry-ext-pg` with `perry-stdlib`'s turnloop HTTP client on 2,
`perry-ext-mysql2` with `perry-ext-fastify` on 4, and `perry-ext-ioredis` with
`perry-stdlib`'s framework server on 5. Reaching one needs a program linking
both bindings — a fastify app that uses mysql2 — so no test caught it.

The runtime now refuses a duplicate, which turns silent misrouting into a clean
decline. This gate is the other half: it makes the collision fail the BUILD
rather than quietly cost one binding its transport at runtime.

Slot numbers live in two places by design — each binding's own crate, and
`perry-db-turnloop::subsystem` for the database band — so this reads both and
resolves the indirection.
"""
from __future__ import annotations
import re, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ROOT / "crates"

# `pub const PG: u8 = 9;` in the database ledger.
NAMED = re.compile(r"^\s*pub const ([A-Z][A-Z0-9_]*)\s*:\s*u8\s*=\s*(\d+)\s*;", re.M)
# `const SUBSYSTEM: u8 = 7;` or `= subsystem::PG;` in a binding.
SLOT = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?const ((?:[A-Z][A-Z0-9_]*_)?SUBSYSTEM)\s*:\s*u8\s*=\s*"
    r"(?:(\d+)|(?:\w+::)?([A-Z][A-Z0-9_]*))\s*;",
    re.M,
)

def main() -> int:
    ledger_path = CRATES / "perry-db-turnloop" / "src" / "lib.rs"
    ledger = {}
    if ledger_path.exists():
        ledger = {n: int(v) for n, v in NAMED.findall(ledger_path.read_text(encoding="utf-8"))}

    # Every `const <NAME>: u8 = <n>;` per crate, so an alias inside a crate can
    # be resolved to the slot it names instead of reading as unresolved.
    same_crate: dict[str, dict[str, int]] = {}
    for path in sorted(CRATES.glob("*/src/**/*.rs")):
        crate = path.parts[len(CRATES.parts)]
        for name, literal, _sym in SLOT.findall(path.read_text(encoding="utf-8")):
            if literal:
                same_crate.setdefault(crate, {})[name] = int(literal)

    claims: dict[int, list[str]] = {}
    unresolved: list[str] = []
    for path in sorted(CRATES.glob("*/src/**/*.rs")):
        # Test slots are deliberately separate and may repeat across test
        # modules; they never register in a shipped binary.
        if path.name in {"tests.rs"} or "/tests/" in path.as_posix():
            continue
        rel = path.relative_to(ROOT).as_posix()
        for name, literal, symbol in SLOT.findall(path.read_text(encoding="utf-8")):
            if literal:
                value = int(literal)
            elif symbol in ledger:
                value = ledger[symbol]
            elif symbol in same_crate.get(path.parts[len(CRATES.parts)], {}):
                # An alias re-exporting this crate's own slot under another
                # name. It is the same slot, so record it once, not twice.
                continue
            else:
                unresolved.append(f"{rel}: {name} = {symbol} (not in perry-db-turnloop::subsystem)")
                continue
            claims.setdefault(value, []).append(f"{rel}: {name}")

    collisions = {v: who for v, who in claims.items() if len(who) > 1}

    for slot in sorted(claims):
        marker = "  <-- COLLISION" if slot in collisions else ""
        print(f"  slot {slot:>2}: {'; '.join(claims[slot])}{marker}")

    if unresolved:
        print("\n::error::subsystem slot could not be resolved:", file=sys.stderr)
        for u in unresolved:
            print(f"  {u}", file=sys.stderr)
        return 1
    if collisions:
        print("\n::error::two bindings claim the same turnloop sink slot.", file=sys.stderr)
        for slot, who in sorted(collisions.items()):
            print(f"  slot {slot}: {' AND '.join(who)}", file=sys.stderr)
        print(
            "\nEvery completion for that slot is routed to whichever binding registered\n"
            "last, which reads the token's low bits as one of its own connection ids.\n"
            "Pick a free slot below MAX_SUBSYSTEMS; the database band is\n"
            "perry-db-turnloop::subsystem and the full map is documented in\n"
            "crates/perry-runtime/src/turnloop_net/sink.rs.",
            file=sys.stderr,
        )
        return 1
    print(f"\nOK: {len(claims)} turnloop sink slots, all distinct.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
