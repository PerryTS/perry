### Fixes

- Per-unit whole-program facts finish on opencode (6,877 modules) again. On
  main the facts stage passed 43 GB in its third round and never converged
  under the 52 GB heavy cap; it now converges in 5 rounds with a 22.4 GB
  facts-stage peak (31.1 GiB for the whole compile). Six mechanisms, each
  fixed by rule:
  - A cell reader wrote back what its seed delivered: every reader of `cjs:*`
    (2,871 CommonJS wrappers) published the registry's 1,929 writes as its
    own. A unit's cell write is now what it writes itself; the cells still
    grow monotonically (DECISIONS 197).
  - Per unit, a CommonJS record's `parent` link read the registry union
    `cjs:*`. The requiring module's record is another unit's value: ⊤, as
    all other units' code is.
  - `global:*` was published ⊤ for global writes the unit only replayed from
    other units (`Model::global_written_own`).
  - Per unit, a computed `require` read the registry union. Its result is
    another module's exports, another unit's value: ⊤.
  - Every unit with dynamic code in reach re-escaped every other unit's thrown
    stand-in arrays through cell `thrown` (27M cell writes in round 3). A
    value another unit threw escapes in its thrower; the reader holds a
    stand-in only (`Model::thrown_in`, taken at seed time).
  - Function and class contract names used a shortened module path, so two
    versions of one package (jose 6.0.11 / 6.2.9), or files of one name in
    two directories, published one contract name twice. Their 13 readers
    were re-solved every round, so the solve never converged. Displays now
    name the full module path.
- `PERRY_FACTS_REPORT=1` prints a census line per dependency level and per
  round of the per-unit driver (what is held and published, and the RSS).
