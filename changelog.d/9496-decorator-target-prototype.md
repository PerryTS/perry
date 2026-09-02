### Fixed

- **A legacy decorator on an instance member now receives `Class.prototype`
  as its `target`, and `Class.constructor === Function` (#9467).** Two
  divergences that cancelled, found while fixing #9404 (#9472). Perry handed
  every member decorator — property, method, method parameter — the class
  itself, where tsc's `__decorate([...], C.prototype, key, desc)` hands an
  instance member's decorator `C.prototype` (a static member's decorator does
  get the constructor). And `C.constructor` answered `C`: the reflective
  decl-prototype carries `constructor` as an ordinary data field, and the
  constructor-side chain walk in `resolve_proto_chain_field` returned it
  before the class-ref read's existing `constructor → Function` tail fallback
  was reached. NestJS-style `Reflect.defineMetadata(k, v, target.constructor)`
  only landed on `C` because both were wrong, so fixing either alone broke
  decorator metadata.

  Both halves land together. `lower/decorators.rs::member_decorator_target`
  hands instance members `PropertyGet(ClassRef, "prototype")` — the same
  decl-prototype object every other `C.prototype` read answers with, so
  `target === C.prototype` and `target.constructor === C` — and statics keep
  the `ClassRef`; `design:type` / `design:paramtypes` ride the same target.
  The metadata store's prototype→class fold is untouched, so the historical
  `getMetadata(..., Class, prop)` reads keep resolving. On the runtime side the
  constructor-side walk skips the `constructor` key alongside
  `class_instance_has_member`, so `C.constructor` falls through to `Function`
  while `C.prototype.constructor` and `(new C()).constructor` stay `C`.

  Fixture `test_decorators_target_prototype_9467` (expected output from
  `tsc --experimentalDecorators --emitDecoratorMetadata` + `reflect-metadata`
  under node) pins the target per member kind, the three `constructor`
  identities, `"constructor" in C` / `hasOwnProperty`, and `Reflect.getMetadata`
  round-trips through `target`, `target.constructor` and inheritance.
