Fixed a moving-GC hazard in `RegExp.prototype.exec` and `String.prototype.matchAll`.
RegExpBuiltinExec coerces `lastIndex` with `ToLength`, which runs user
`valueOf`/`toString` when `lastIndex` holds an object — arbitrary JS that can
run a copying minor. Both entry points captured the subject string (and, in
`exec`, borrowed its inline payload) *before* that coercion, so a collection
inside the callback left the match running over relocated-away bytes: the match
silently evaporated or returned text from unrelated heap memory. The coercion
now runs first with the regex header and subject rooted, and every later use
reads the refreshed addresses — including the match array's `.input` property
and the `lastIndex` write-back on the lookbehind/backreference path.
