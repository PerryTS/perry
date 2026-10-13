# wpfacts adversarial corpus (slice 1, design 6.3)

One program per escape source. Each one tries to break a fact the analysis
could otherwise claim. A test passes only when BOTH hold:

1. the program prints exactly what `node` prints (normal build and the
   `PERRY_CHECK_CONTRACTS=1` contract build), and
2. its facts dump shows the fact OFF (the `// WPFACTS:` lines).

`p00_positive.ts` is the positive control: the same shapes with nothing
breaking them, where the facts must be ON (a corpus that only ever sees
OFF could not tell a working analysis from one that claims nothing).

Assertion lines (`// WPFACTS: ...`):

- `fact <F-code> <fact> <Name>[.<key>] = <0|1>` — a `FACT` line of the dump
  (`Name` matches a template display's `:Name` suffix);
- `kind <Name>.<key> has <prim>` — the field's kind includes `<prim>`;
- `site .<key> off|on` — every non-call read site of `<key>` in the entry
  file is (not) proven Tier 0;
- `call .<key> off|on` — the same for method-call sites;
- `global on|off` — the program-wide E14 kill.

Run: `python3 scripts/wpfacts_corpus.py --perry <perry> --out <dir>`.
