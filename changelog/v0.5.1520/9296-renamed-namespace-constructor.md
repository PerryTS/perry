### Fixed

- **Classes constructed through renamed namespace re-exports now receive their
  constructor arguments (#9285).** `new ns.PublicChild(value)` resolves the
  class metadata from its defining export while retaining the namespace-visible
  alias, so constructor parameter properties and other initialization run
  normally without sacrificing collision-safe namespace class identity.
