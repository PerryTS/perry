`--target web`: `new Date(x)` returned an undefined value, so `getTime()` and
every getter read 0 and `toISOString()` was undefined. The emitter called a
`date_new` bridge that was never interned, which `mem_call` resolved to the
wrong name; it now calls `date_new_val`, which also forwards the argument.
The Date bridges the runtime implements but the interned list lacked
(`Date.parse`, `Date.UTC`, `valueOf`, the UTC getters and setters,
`getTimezoneOffset`, the `to*String` family, `toJSON`) are interned too.
