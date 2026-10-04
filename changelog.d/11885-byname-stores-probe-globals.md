Stores, literal definitions, builtin globals and `fn.bind`/`call`/`apply`
are answered from shapes instead of by name (#10495, #10497).

- An object literal that takes the source-ordered path (a spread, an
  accessor, a computed key or a method) now DEFINES its properties
  (CreateDataPropertyOrThrow) instead of assigning them: an
  `Object.prototype` setter or read-only property of the same name no longer
  runs or rejects, and a key an earlier part made an accessor becomes a data
  property, as in node. The definition is answered from the receiver shape:
  the key-add edge for a key the shape lacks, the own slot for one it holds.
  `{ ...src }` copies a plain source by position when its shape proves every
  key an enumerable data property, and defines each key the same way.
- `o[k] = v` on a plain object reaching the generic store
  (`js_put_value_set`, e.g. from `js_dyn_index_set_strict`) takes the
  key-add edge before the full `[[Set]]` walk, as the cached computed-store
  site already did on its miss.
- A public class field defined on an instance the constructor's inline stores
  do not cover is answered from the receiver shape before the key list is
  scanned by name; a declared field the constructor already listed under its
  (not interned) literal key is found by content and overwritten in place.
- A builtin global identifier (`Object`, `Array`, `Map` ...) is a static read
  site of the global object with the name's pooled key: the per-site cache
  compares the global object's ShapeId and loads the slot, instead of
  validating, interning and looking up the name on every evaluation. A
  reassigned or redefined global is seen by the next read.
- `fn.bind(...)`, `fn.call(...)`, `fn.apply(...)` on a function that inherits
  them from `%Function.prototype%` are answered from a method-site entry: the
  receiver's word, `%Function.prototype%`'s word, the slot and the
  intrinsic's body info, all compared on every call. A patched slot, an own
  shadowing property or a redefined key fails a compare and takes the full
  dispatch. The method name is read only when the entry is primed, and only
  a receiver whose ShapeId inherits from `%Function.prototype%` asks for an
  entry at all.

Real programs (instructions:u, n=5, output equal to node, full collections
unchanged): Zod x5000 19.84 G -> 16.94 G (-14.6%), qs parse_nested 33.10 G ->
29.65 G (-10.4%), commander parse_argv 8.88 G -> 8.22 G (-7.5%), tsc x1
60.716 G -> 60.713 G (-0.01%); peak RSS lower on all four (tsc 319.3 -> 310.9 MB).
Per operation (instructions:u, main -> this change): `mono[i].constructor === Object`
1,339 -> 240 (node 31); `getPrototypeOf(o) === Object.prototype` 3,456 -> 2,353;
a Zod `create` literal (three keys and a two-key spread) 34,629 -> 12,789;
`new E()` for `class E extends EventEmitter` with four fields 37,376 -> 35,169.
