### Fixed

- **A class expression returned from a factory function had no
  per-evaluation identity when the function lived in a non-entry module**
  (#10455). `function withCommands(Base) { return class extends Base {}; }`
  called twice with different `Base`s returned the SAME class object,
  re-parented to the most recently passed `Base` — declared and called in
  the entry module, the identical shape already worked because
  `specialize_captured_class_factories` (`crates/perry-transform/src/inline/factory_specialize.rs`)
  clones a distinct class per call site, but that pass only ever sees call
  sites in the SAME module the factory Call expression appears in; a caller
  in another module reaches the factory through an ordinary cross-module
  call the pass never visits. redis's `commander.js` `attachConfig` (`Class
  = class extends BaseClass {}`) hits this every time it's called with a
  second `BaseClass`, from `RedisClient.factory` and
  `Client.prototype.Multi`, and `new Client(options)` ran the wrong parent
  constructor. New pass `fresh_export_dynamic_heritage_factories`: for an
  **exported** factory whose body is exactly `return class extends <expr>
  {};` (no other members), upgrades the class's shared-template `ClassRef`
  to a fresh-per-evaluation `ClassExprFresh` object directly — exactly what
  the same class expression would already lower to had it needed
  per-evaluation statics/captures/a private brand — so every caller, local
  or cross-module, gets a genuinely distinct class per call. Scoped to
  exported functions and this single-statement shape only; a `Let`-bound
  intermediate variable or Effect's object-literal wrapper factory shape
  still rely on same-module specialization alone.
