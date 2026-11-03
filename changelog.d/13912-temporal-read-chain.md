A by-name read on a Temporal value (`d.constructor`, `d.hasOwnProperty`, a
member added to `Temporal.PlainDate.prototype`) answered `undefined` when the
built-in getters and methods did not know the key. It now continues as the
ordinary property read on `Temporal.<Kind>.prototype`.
