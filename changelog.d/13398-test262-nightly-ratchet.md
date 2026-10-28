### Added

- Nightly Test262 coverage: `scripts/test262_subset.py --full-corpus` scores every Test262 case by Perry alone (comparable to other engines' published figures), and the full-tier `test262` / `test262-aggregate` jobs ratchet it per test against `test-compat/test262/baseline.json` (88.84%, 47,346 / 53,293).

### Fixed

- Self-validated `async` Test262 cases that failed through `$DONE` were scored as passes.
