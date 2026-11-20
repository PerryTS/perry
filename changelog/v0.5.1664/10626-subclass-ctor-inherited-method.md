### Fixed

- **A subclass constructor assigning `this.m = ...` over an inherited method
  hid that method from the moment `super()` returned, not just before the
  assignment ran.** The constructor-body field-detection pass excluded a
  class's OWN methods from being turned into shadow data slots (so
  `this.parse = this.parse.bind(this)` self-binding kept working), but never
  excluded methods inherited from the `extends` chain — so
  `this.close = () => ...` in a subclass constructor, where `close` is
  declared only on a parent class, allocated an own `close` field that
  existed (as `undefined`) as soon as `super()` returned and shadowed the
  inherited method in every by-name lookup until the assignment statement
  executed. Instance method names are now tracked as an own+inherited union
  per class (mirroring the existing accessor-name tracking) and consulted the
  same way. This was blocking `undici`'s `MockPool`/`MockClient`
  (`this[kOriginalClose] = this.close.bind(this)` over `DispatcherBase`'s
  `close`), which threw `TypeError: Bind must be called on a function`.
