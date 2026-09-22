### Fixed

- **A generic `Object.defineProperty`/`defineProperties` descriptor (no
  `get`/`set`/`value`/`writable`, e.g. `{ enumerable: true }`) against an
  existing **class-declared** get/set accessor no longer breaks it.** (#10480)
  A ClassBody accessor lives in the class vtable, not the address-keyed
  descriptor tables `defineProperty` normally writes, so the generic-descriptor
  branch could not see the class key: it appended a shadowing data property
  with `writable: false`, which silenced the setter (assignment threw in
  strict code, silently dropped in sloppy code) and never actually applied the
  requested `enumerable`/`configurable` change. Every WebIDL-generated class
  (whatwg-url, node-fetch, undici-style polyfills) marks its prototype
  accessors enumerable exactly this way at module load — node-fetch's
  `Object.defineProperties(Request.prototype, { method: { enumerable: true },
  … })` broke every later write to those accessors. A new per-accessor
  side table (`class_registry/accessor_attrs.rs`) now records the overridden
  attributes instead, so `getOwnPropertyDescriptor`, `Object.keys`/`values`/
  `entries`, `hasOwnProperty`/`propertyIsEnumerable`, and `delete` all see the
  update while the getter/setter stay exactly where they already lived.
