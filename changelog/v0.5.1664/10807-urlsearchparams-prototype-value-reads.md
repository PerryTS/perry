### Fixed

- **`URLSearchParams.prototype` methods read as a value now resolve to real,
  callable closures.** `URLSearchParams.prototype.append`, `.prototype["has"]`,
  and the same reads through a Proxy `get` trap previously returned
  `undefined`, so `Function.prototype.call`/`.apply` on the result threw
  `Function.prototype.call was called on a value that is not a function`.
  node-fetch@3.3.2's `Headers extends URLSearchParams` hits this via a
  constructor-returned `Proxy` whose `get` trap does exactly this, on every
  `fetch()` call. The six other `#10555`-group members
  (`AbortController`, `AbortSignal`, `CustomEvent`, `Event`, `EventTarget`,
  `URL`) have the identical defect and remain unfixed — see #10807's PR body
  for the audit. (#10759)
