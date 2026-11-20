### Fixed

- **Error subclasses no longer expose their default `name` as an own,
  enumerable property.** `name` now remains on the appropriate Error-family
  prototype until user code explicitly assigns it, matching Node across
  `JSON.stringify`, `Object.getOwnPropertyNames`, `Object.keys`, `for…in`,
  object spread, property descriptors, and `util.inspect`. Error construction
  also preserves Node's observable own-key order: `stack` precedes an optional
  `message`.
