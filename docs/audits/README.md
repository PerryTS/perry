# Audits and validation records

Point-in-time evidence from specific issues and PRs: measurements, investigations and validation runs. These are not maintained documentation; each file reflects the commit it was measured on. Current behavior is documented in `docs/src/`.

Nothing outside this directory links to these files except `scripts/check_gc_env_knobs.py`, which lists `statepoints-only-12023.md` as a historical document. Review this directory periodically and delete records whose work has landed.

| Record | Date | Topic |
|--------|------|-------|
| `gc-rss-instructions-2026-10-03/` | 2026-10-03 | GC RSS and instruction-count measurement campaign (scripts, inputs, tables) |
| `native-stream-rooting-2026-10-04.md` | 2026-10-04 | Native stream pipeline rooting audit |
| `native-payload-lifecycle-rss.md` | 2026-10-05 | Lifecycle RSS attribution (#11919) |
| `native-payload-lifecycle-validation.md` | 2026-10-05 | Native payload lifecycle runtime validation (#11919) |
| `native-payload-zlib-validation.md` | 2026-10-07 | Native payload zlib validation (#11919) |
| `statepoints-only-12023*.md`, `statepoints-only-12023-inventory.txt` | 2026-10-06 | Statepoint lowering investigation and acceptance (#12023) |
| `perf-12015-attribution.md` | 2026-10-06 | Object metadata and enumeration attribution (#12015) |
| `design-lazy-literal-methods.md` | 2026-10-08 | Lazy object-literal methods (design note, not built) |
| `actions-consolidation-plan.md`, `actions-consolidation-inventory.json` | 2026-10-08 | GitHub Actions consolidation plan |
| `2026-10-11-stdelete.md` | 2026-10-11 | Class D side-table deletion |
| `wasi-component-validation-2026-10-21/` | 2026-10-21 | WASI and component-model validation runs |
