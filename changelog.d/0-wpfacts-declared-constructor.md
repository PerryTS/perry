Whole-program facts: `new Event(type, options)` and `new CustomEvent(type,
options)` no longer hand their arguments to unknown code. The runtime now
declares what these constructors do (ToString of the type; a [[Get]] of
`bubbles`, `cancelable`, `composed` and, for CustomEvent, `detail` on the
options; a fresh event that holds `detail`), and the solver runs that
declaration while the global still holds the realm constructor. Program
getters on the options and a `toString` on the type run as in Node; a
`detail` object escapes exactly when the event does. When the program writes
`globalThis.CustomEvent` (or `Event`), the arguments escape as before. In
three.js this was the second link of the chain that hands every math class to
unknown code (`dispatchEvent(new CustomEvent("observe", { detail: this }))`).
