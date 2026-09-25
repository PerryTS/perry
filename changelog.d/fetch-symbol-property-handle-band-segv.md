Fixed `Reflect.set`/`obj[sym] = v` segfaulting on a native Web Fetch handle
(`Request`/`Response`/`Headers`) — e.g. `Reflect.set(new Request(url), Symbol.for("x"), v)`,
the pattern Astro's `@astrojs/node` adapter uses to tag every request with its
client address. `set_symbol_property`'s frozen/non-extensible guard (and its
`symbol_property_is_non_writable` mirror) gated the `GcHeader` dereference on a
hand-typed `obj_key >= 0x10000` floor — an order of magnitude below
`HANDLE_BAND_MAX` (0x100000) — paired with `is_valid_obj_ptr`, whose doc
explicitly warns it does *not* reject the handle band by itself. A Web Fetch
handle id (`[0x40000, 0xE0000)`) sailed through both checks and
`(obj_key - GC_HEADER_SIZE) as *const GcHeader` read unmapped memory. Both call
sites now use `try_read_gc_header`, which classifies the whole handle band
(and alignment) before any dereference and returns `None` for handle
receivers — the same guard already used elsewhere in the runtime for this
exact class of bug (#1843/#4004/#4800).

Test: `test-parity/node-suite/fetch/symbol-property-set.ts` (Reflect.set/get,
direct `obj[sym]`, overwrite, `Object.getOwnPropertySymbols`, delete, and an
untyped-parameter write, on `Request`/`Response`/`Headers`).
