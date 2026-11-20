A `/…/` literal lowers to `js_regexp_new` at its evaluation site, and
`js_regexp_new` answered "is this a SyntaxError?" by BUILDING the pattern —
`regex_syntax` parse, HIR translate including Unicode case folding, Thompson
NFA construction, meta-engine strategy selection. Every regex a program *had*
was compiled, not every regex it *used*. A symbolized instruction profile of
the claude-code CLI running `--help` — a command that prints text and exits —
put 14.6% of all retired instructions inside regex compilation, against 0.11%
for the whole of its compiled JavaScript.

Only the program build is deferred; everything observable at construction stays
at construction. A syntactically invalid pattern still throws `SyntaxError`
from `js_regexp_new` / `RegExp.prototype.compile`, at the same point in the
program, because `std_engine_syntax_ok` runs the same parser `build_std_regex`
would run, on the same translated, flag-prefixed, REDoS-collapsed string —
4.6 µs/pattern against 82 µs to build. Anything it rejects falls through to the
unchanged both-engines check, so the fancy-regex fallback for
lookbehind/backreferences still decides and still throws when both refuse.
`.source`, `.flags`, `.global`, `.sticky` and `lastIndex` never touched the
compiled program, and identity is unchanged. The build happens on the first
operation that needs a matcher.

| literals constructed, 1 used | before | after | node |
|---|---|---|---|
| 50 | 19 ms | 2 ms | 0 ms |
| 200 | 73 ms | 7 ms | 1 ms |
| 400 | 145 ms | 15 ms | 3 ms |
| every distinct literal in cli_2.1.112.js (2,378) | 232 ms | 50 ms | 5 ms |

Wall clock on the cc corpus 247.5 → 59.9 ms.
