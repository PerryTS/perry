### Performance

- Preserve compiler-published class-instance shapes while initializing declared
  public fields. When `CreateDataProperty` targets an existing ordinary own data
  slot with default attributes, Perry now uses the validated barriered overwrite
  path instead of allocating a descriptor and marking the instance
  descriptor-bearing. Proxies, exotics, custom descriptors, missing keys, and
  unsupported key representations retain the complete `[[DefineOwnProperty]]`
  path.

- Scope keyed prototype-mutation invalidation to the affected method name rather
  than permanently disabling every direct-method guard in the process. Hash
  collisions fail closed by retiring extra guards, and the process-wide latch
  remains for keyless mutation paths. Generic fallback dispatch now also observes
  deletion, replacement, and re-creation of declared prototype methods.

- On the unchanged full `codehz/ecs` comprehensive suite, 11 alternating Mac
  mini pairs reduced the 10k read-only query median from 7.5397 to 1.2491 ms
  (83.47%) and accumulation from 7.4646 to 1.2288 ms (83.52%), with 11/11 wins
  and every output/checksum oracle passing. This removes persistent suite-order
  guard poisoning; it does not establish Node parity.
