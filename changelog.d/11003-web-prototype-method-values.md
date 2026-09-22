### Fixed

- Web builtin prototype methods on `URL`, `AbortController`, `AbortSignal`,
  `EventTarget`, `Event`, and `CustomEvent` are available as callable values.
  `URL.prototype.toString.call(url)` now returns the URL href, and inherited
  methods resolve through the `AbortSignal` and `CustomEvent` prototype chains.
