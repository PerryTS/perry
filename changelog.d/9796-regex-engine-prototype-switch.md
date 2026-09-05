### Internal

- **`PERRY_REGEX_ENGINE=regress` — a measurable tier-0 engine prototype.**
  Routes every pattern through `regress` (the ECMAScript backtracker perry
  already links for RepeatMatcher capture semantics) instead of only the ones
  whose capture semantics require it, and installs a shared never-match
  placeholder as the standard program so no NFA is built. Every exec-family
  entry point already consults the repeat matcher first, so this exercises the
  whole engine surface — `exec`, `test`, `match`, `matchAll`, `search`,
  `split`, `replace` — without a second implementation.

  It exists so the engine question is settled on measurements from a real
  binary rather than on a corpus harness. Measured over 4,463 distinct regex
  literals extracted from seven real bundles (two claude-code builds, ethers,
  moment, dayjs, luxon, mongodb) with a tracking allocator and the programs
  held live:

  | engine | accepted | compile µs (med) | bytes/program (med) | corpus total |
  |---|---|---|---|---|
  | `regex` crate (tier 1 today) | 92.3 % | 48.5 | 12,492 | 136.7 MB |
  | `regress` | **100 %** | **2.2** | **512** | **4.9 MB** |
  | `fancy-regex` (tier 2 today) | 97.8 % | 59.2 | 12,623 | 146.6 MB |

  node/V8, measured the same session, is ~2,600 bytes per program. A
  differential over 4,119 patterns × 13 subjects (53,547 comparisons of match
  presence, span and every capture span) found **0 disagreements** between the
  linear engine and `regress`.

  **Not a supported configuration**: the backtracker has no step budget, so a
  pathological pattern can run unbounded. Off by default, one relaxed atomic
  load when unset.
