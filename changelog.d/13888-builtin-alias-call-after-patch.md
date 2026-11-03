A builtin method captured before the program replaced it
(`const origPush = Array.prototype.push; Array.prototype.push = function (...xs) { origPush.call(log, ...); ... }`)
recursed until the stack overflowed when the replacement called it through
`.call`/`.apply`: the compiler rewrote `origPush.call(r, x)` to `r.push(x)`,
which reads `push` by name again and so reaches the replacement. That rewrite
no longer applies once the program writes that method name (or `call`/`apply`)
onto a builtin prototype; the reflective call runs the value it names.
