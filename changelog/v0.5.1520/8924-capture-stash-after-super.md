Place a derived class's early capture stash (`this.__perry_cap_* = param`)
after the statement that completes `super()` even when the call is not its own
statement — a minifier's `super(a), this.x = b, …` comma sequence (split so
the stash sits right after the call), an `if (super(), …)` test, or a `try`.
The stash used to fall back to constructor entry, which #8643's derived-`this`
TDZ turned into `ReferenceError: Must call super constructor in derived class
before accessing 'this' or returning from derived constructor` at every
construction — Coop's Next.js App Route fixture died at module init on
`new AppRouteRouteModule({…})` (refs #8546, #8882).
