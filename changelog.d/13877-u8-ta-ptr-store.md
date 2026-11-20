A `Uint8Array` argument now stores through the typed-array spec ABI instead of
the guarded byte path (#13777 landed the read half). `a[i] = v` on a `TaPtr`
parameter takes the same proven-view checked store `IndexSet` takes: a bounds
compare against the entry-hoisted length and one `store i8`, with no per-store
receiver guard and no `js_uint8array_set` slow arm.
ToUint8 needs no new conversion gate: that tier admits an integer element only
for a proven exact-i32 value, whose truncated low byte is ToUint8, so a value
that is not proven integral (`v + i` from a `Number` parameter, `NaN`, a
string) keeps the guarded store, and a store past the end stays the silent
no-op the spec requires.
