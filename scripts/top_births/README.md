# TOP birth census (diagnostic only)

Enable `PERRY_FACTS_DUMP=<file> PERRY_FACTS_UNITS=0 PERRY_FACTS_BIRTH_CENSUS=1` on a facts run. The census observes the final solver model, including initialization. It records repeated injection attempts, all TOP nodes, copy/rule transfers, escape/TopSink registrations, native write/read gates and summary-slot origins. It does not use type annotations to decide facts.

A birth is a destination node with at least one explicit TOP injection independent of a TOP predecessor. All independent injector locations at that node share one origin bit; `birth-injectors.tsv.gz` lists every injecting rule and reason. TOP-induced escape inputs and written-native reads become dependent edges, not fresh origins. Independent opaque native outputs and external callback inputs remain births. This is observed monotone forward reachability, not a claim that removing one root proves every reachable site. The diagnostic ceiling measures that separately.

`PERRY_FACTS_SUPPRESS_BIRTH=<Rust file>:<line>:<node>` skips that injector only with `PERRY_FACTS_ONLY=1` and the census enabled. It never generates an executable from suppressed facts. Node IDs belong to the observed final model; selectors do not apply to intermediate liveness models. With the census off, ordinary solver outputs and generated runtime code are unchanged.

## Analyze

Set `PERRY_BIRTH_OUTPUT` to a directory containing `NAME.dump` and `NAME.ledger.tsv.gz`. The ledger must have `file,start,end,kind,key,verdict`, with `verdict=unproven` for sites to count. Set `PERRY_BIRTH_SOURCE_ROOT` to the source prefix removed from dump paths.

```
python3 scripts/top_births/inst.py NAME
python3 scripts/top_births/an.py NAME causal
python3 scripts/top_births/sites.py BIRTH_ID > identities.tsv
python3 scripts/top_births/sites.py BIRTH_ID --sole > sole-identities.tsv
```

`births.tsv` contains every retained birth, including zero-mass birth nodes, ranked by sole then total mass. IDs are stable within a census. A node can have multiple injection kinds; kind counts overlap. `switch` is populated only for a node with one independent injector; the measured top three all meet that condition. `birth-summary.json` includes top-25 lists, kind counts, graph coverage and association audits. `birth-sites.tsv.gz` maps exact site identities to `birth-masks.tsv.gz`; together they encode the set of sites for **every** birth without duplicating the common SCC for each one. `birth-locations.tsv.gz` gives all associated expressions. A shared source/model node may represent multiple source expressions; `source_count` exposes that multiplicity. A generated operation without an exact source expression is labelled `model-generated` with its actual Rust injection location, never an invented TypeScript location.

For syntax-validated expression labels, `source_ast.mjs` accepts `source-request.json` with `files`, `globals`, `modules`, and `methods` arrays; set `PERRY_BIRTH_TYPESCRIPT_MODULE` to the TypeScript JS module. It parses syntax only, excludes type-only imports, and never consults a type checker. This optional source overlay does not affect the causal graph or masses.

The graph condenses SCCs before propagating root bit sets. Identity views are deduplicated by the ledger key; mass includes each identity once per reachable birth. Roots reaching an unproven callee/receiver/index TOP, or the observed escape event of its receiver, are included. Escape-only blockers with no upstream TOP birth have an empty mask. The summary separately reports those rows and every retained association gap. Never silently call a gap a proven source. Delete indexed dump intermediates after review; they are analysis artifacts, not facts inputs.

## Leak count (one command)

`scripts/top_births/leaks.sh DIR NAME SOURCE_ROOT` runs `inst.py` and `an.py NAME causal` on `DIR/NAME.dump` + `DIR/NAME.ledger.tsv.gz` and prints a TSV: `TOTAL` (birth nodes), `injector_locations`, then one row per injection kind, largest first. Kinds overlap (a node injected by several kinds counts under each); `TOTAL` counts each node once. `leaks.py DIR` alone reprints the table from an existing `birth-summary.json`. A rule PR reports this table before and after; the rows named `other: file:line` are injection sites the classifier in `an.py` has no name for yet.

## First opener per gate

A native-write gate (a builtin prototype, `⊤`, a namespace or every builtin function value, under one key or every key) opens at its first write; every later write under it is a no-op for the solver, so the census can name only the write that opened it. `birth-opener` records it once per gate, in opening order (`seq`): the holder, the key (`null` = every key), what wrote it, the store site and its function, and the receiver node with the root of its ⊤ chain (`recvRoot`, `recvWhy`). `openers.py NAME.dump [KEY]` prints them as a TSV (with KEY: only the gates a read of KEY passes, the first row opened it); `leaks.sh` writes `NAME.openers.tsv`. The holder `builtin-fn` with every key is the write behind "property of a written builtin function value".

## Arrival order

Each first-cause record (a node's predecessor, an injection, a token's escape-first, a gate opener) names one cause, but not when it happened, so a chain through several record kinds can close a loop. The census therefore runs one clock (`seq`) over the events that may cause a later one: every gate opener (`birth-opener`), every node's first ⊤ (`birth-arrival`: its copy source, or `injected` with the gate it replays, plus the rule's node, call and escaping parent token) and every token's first escape (`seq` on `escape-first`). `chain.py NAME.dump START...` walks back from START (`tok:N`, `node:N`, `gate:HOLDER:KEY`, or a token name suffix such as `three.core.js:Vector3`) to the earliest cause with a smaller `seq` at every step, so it ends at the seed (an injection, with its injector line) or at an event with no earlier recorded cause. Census off: no `birth-arrival`, `seq` null.

## Checks

```
python3 tests/tooling/infrastructure/top_births/test_analysis.py
python3 tests/tooling/infrastructure/top_births/test_causal.py
python3 tests/integration/harnesses/top_births/test_capture.py --perry /path/to/perry --runtime /path/to/release --workspace /path/to/worktree
python3 tests/integration/harnesses/top_births/test_openers.py --perry /path/to/perry --runtime /path/to/release --workspace /path/to/worktree
python3 tests/integration/harnesses/top_births/test_arrival.py --perry /path/to/perry --runtime /path/to/release --workspace /path/to/worktree
```

The capture check is red on a pristine compiler without the feature. It asserts a live recorded birth, checks that its suppression actually removes TOP, and checks the census-off arm. The graph checks cover SCCs, shared causes, identity deduplication, weighted mass, escaped-input relabelling and native gate dependencies.

CXT1's measured KLNT run, exact environment, reference/ceiling ledgers, scripts and complete report live in `/root/CXT1` on qb9. `run.sh` and `cmp.py` there preserve the reference harness and proven-by-identity comparisons. They are environment-specific and are not production compiler entry points.
