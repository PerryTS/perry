No-auto builds take the stream dispatch features of the coherent
runtime/stdlib/wrapper rebuild, and the wrapper archives they link, from the
routed modules, the same selection automatic specialization uses. The rebuild
used to be an HTTP-only special case that also linked perry-ext-http for any
`node:http` import, even when PERRY_DISABLE_WELL_KNOWN=1 served it from the
bundled libraries.

Codegen and the linker now read one routing decision for native modules
(`NativeRouting`, computed once per compile from the well-known bindings and
PERRY_DISABLE_WELL_KNOWN). A module's namespace installs its wrapper's hook
only when the module is routed to that wrapper, and static wrapper calls are
emitted only for routed wrappers; otherwise the runtime dispatch bucket and
by-name dispatch serve it. With PERRY_DISABLE_WELL_KNOWN=1 a `node:http`
import therefore no longer names perry-ext-http symbols that the link does
not include. `zlib` joins `net` and `ws` as a module whose wrapper is its only
provider, so it routes to perry-ext-zlib in every mode.
