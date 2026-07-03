#!/usr/bin/env bash
source "$(dirname "$0")/_perry_test_lib.sh"

# A `switch` on a BigInt must match `case 1n` by value, not allocation
# identity — the same rule the Set/Map key and `===` paths already follow.
perry_run main.ts <<'TS'
function classify(x: bigint): string {
  switch (x) {
    case 0n: return "zero";
    case 1n: return "one";
    case 9007199254740993n: return "big";
    default: return "other";
  }
}
console.log(JSON.stringify({
  computed: classify(1n + 0n),
  big: classify(9007199254740992n + 1n),
  miss: classify(5n),
  mixedType: (1n as any) === 1 ? "eqNumber" : "neqNumber",
}));
TS

perry_expect_node
perry_pass "switch matches BigInt case by value"
