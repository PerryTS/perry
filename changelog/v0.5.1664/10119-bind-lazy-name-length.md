### Performance

- **`Function.prototype.bind` no longer eagerly builds `"bound " + name` or its
  `name`/`length` property-attribute records on every call.** A bind whose
  result never reads `.name`/`.length` now performs neither the runtime-string
  allocation nor the two `set_builtin_property_attrs` side-table inserts —
  redundant, since a closure with no dynamic-prop entry for those keys already
  defaults correctly everywhere it's observed. `.name`'s string is built and
  cached lazily on first actual read, through the same seam every other reader
  of a closure's `.name` already goes through, so `Object.
  getOwnPropertyDescriptor`, `console.log`, and a chained `.bind().bind()` all
  still see the right value. `Get(Target, "name")` still runs synchronously at
  bind time, so a throwing `name` getter on the target still fails `bind()`
  itself. Refs #10084.
