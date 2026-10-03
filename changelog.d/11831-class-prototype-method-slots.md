**runtime/codegen: a class prototype's method slot holds a function object of the method's own body (Refs #10502)**

A declared class's `C.prototype.m` was a name trampoline: a bound-method
closure that re-resolved the body by name (`lookup_class_method_in_chain`) on
every call, and whose slot no shape could describe. Every class instance method
now gets its closure-convention entry (`<method>__eclo`, until now emitted only
for per-evaluation classes), and the method value of a declared class — `c.m`,
`C.prototype.m`, the prototype's slot — is one function object running that
entry. The materialized prototype's shape records the slot as a ConstFn lane
naming the body (step 5C), the first step of retiring the runtime class method
table: prototypes are the objects dispatch will resolve through.

- `f.call(x)` / `C.prototype.m.apply(...)` call the body directly instead of a
  by-name lookup.
- Method objects are no constructors and have no own `prototype`
  (`FN_NON_CONSTRUCTOR`, honored by `extends`, `delete fn.prototype` and
  Proxy delete).
- `String(C.prototype.m)` of a per-evaluation class now prints the method's
  source (it printed the entry's synthesized text).
- Module init registers each method with its entry in the one call it always
  made (`js_register_class_method_with_entry`); the method object's name is
  registered when the object is first built, and the entries are emitted after
  the init chunks. Cost per declared instance method: one `JsFunctionInfo`
  (~290 B of binary, ~30 instructions of load-time relocation), as every static
  method already has.
- Dispatch is unchanged: compiled calls and the runtime dispatcher still use the
  vtable; later slices move them onto receiver shape -> holder shape -> slot.
- `codegen/method_entries.rs` takes the method-entry emitters out of
  `string_pool.rs`, which brings it back under the 2000-line gate.

- Linux (native glibc) links now pass `-Wl,-z,pack-relative-relocs`, so the new
  per-method relocations are stored as a DT_RELR bitmap, not 24-byte RELA
  entries: the compiled binary is smaller than before this change (tsc -1.2%,
  Zod -3.9%, hello -5%). Gated by a cached probe link: the flag is added only if
  `cc` links a test program with it AND the output records the
  `GLIBC_ABI_DT_RELR` version need (glibc >= 2.36 at link time); otherwise the
  link is unchanged. Not used for musl, cross links, Android, HarmonyOS, macOS
  or Windows.
