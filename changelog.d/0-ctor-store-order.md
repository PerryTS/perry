A constructor that adds an instance's keys out of the class layout's order no
longer loses keys. Since class keys are laid out in first-store order
(#14485), a run that adds them in another order (the arms of an `if` storing
two keys in opposite orders, a key a later loop iteration adds, a parent's key
stored late by a subclass) gets the runtime's overflow table, but the facts
still called the keys stored after that point present from birth, so their
stores wrote the slot without adding the key: `Object.keys`, `for...in` and
`JSON.stringify` missed them (or listed them in layout order), and a
subclass's field read `undefined`. Such keys are now created through the
checked store. A store after a `break` or `continue` in a `do...while` body is
no longer taken as always run (the read could return the hole: `v ===
undefined` was false). `PERRY_FACTS_CTOR_ORDER=0` restores the old facts.
