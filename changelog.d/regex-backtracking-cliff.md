### Performance

- **A capture group no longer turns a pattern into a ReDoS.**
  `repeat_matcher::capture_layout` takes a pattern off the linear `regex`
  engine when ECMA-262's RepeatMatcher capture semantics are observable — a
  capture group directly under a quantifier, or a capture inside a negative
  lookaround. That routing is a correctness requirement (the linear engine
  keeps the last value of a capture nested in a quantified group; the spec
  clears it on every iteration), but the engine it routes to, `regress`, is a
  classical backtracker with no step budget. So adding parentheses was enough
  to fall off a linear-time path onto an exponential one:

  | pattern | node | perry (before) | perry (after) |
  |---|---|---|---|
  | `/^(a+)+$/.test("a"×28 + "!")` | 4,798 ms | **16,522 ms** | **0 ms** |
  | `/^(?:a+)+$/.test(…)` (same language, no capture) | 4,288 ms | 0 ms | 0 ms |

  **6.3 %** of the 4,463 distinct regex literals across seven real bundles
  take that route — claude-code 7.1 %, dayjs 25 %, luxon 29 % — including
  shapes like `^[a-z][a-z0-9]*(-[a-z0-9]+)*$`.

  The two engines accept exactly the same LANGUAGE for a pattern they both
  compile; they disagree only about which capture assignment to report. So the
  linear program is asked first (`linear_rules_out_match`), and when it proves
  there is no match at or after the search offset — which is what every ReDoS
  input is, a subject that ALMOST matches and then fails — the backtracker is
  never entered. Every `&str`-subject entry point goes through
  `lookup_repeat_matcher_for`: `test`, `exec`, `match`, `matchAll`, `search`,
  `split` and `replace` with a string replacement. The gate disables itself
  where the linear engine has no opinion (a pattern it could not compile holds
  the never-match placeholder), which is exactly the lookaround shapes.

  **This removes the reachable exponential case; it does not BOUND the worst
  case.** A real step budget has to be counted by the backtracker, and
  `regress` has none today (`fancy-regex`, by contrast, ships
  `backtrack_limit: 1_000_000`). A 101-line patch adding one has been measured
  — worst hostile search 51 s → 124 ms at a budget of 1,000,000, zero answers
  changed across 13,389 real searches, upstream's own 544 tests unchanged — and
  is open upstream as
  [ridiculousfish/regress#177](https://github.com/ridiculousfish/regress/pull/177).
  Until it lands and perry picks it up, do not read "cliff fixed" as "worst
  case bounded".
  (`quantified_capture_pattern_does_not_backtrack_on_a_non_matching_subject`)
