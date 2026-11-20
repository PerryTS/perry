A receiver annotated with the generic spelling of a typed array —
`m: Uint8Array<ArrayBuffer>` — silently got the wrong `indexOf` /
`lastIndexOf` / `includes` / `join` (#10894).

TypeScript 5.7 made every typed array and `DataView` generic over the
backing buffer, and `@types/node` did the same for `Buffer`; `tsc` now
*prints* the argument (`new Uint8Array(n)` is `Uint8Array<ArrayBuffer>`),
so it is what people copy into annotations. The argument has no runtime
meaning, but HIR lowered `X<…>` as `Type::Generic { base: "X" }` while every
typed-array / buffer recognizer keys on `Type::Named("X")`. The value then
read as "known not a string, not a typed array" and the Array fast path
folded `m.indexOf(44, from)` to `Expr::ArrayIndexOf` over a `BufferHeader`:
the payload was searched eight bytes at a time as NaN-boxed values, and a
byte that was present came back `-1` — with `length` and `m[i]` still
correct beside it.

Measured against node v26.5.1 on the same six-byte `Uint8Array`:

| annotation | `indexOf(44) indexOf(44,2) lastIndexOf(44) includes(200)` | before | after |
|---|---|---|---|
| `Uint8Array` | `1 3 3 true` | `1 3 3 true` | `1 3 3 true` |
| `Uint8Array<ArrayBuffer>` | `1 3 3 true` | **`-1 -1 -1 false`** | `1 3 3 true` |
| `Uint8Array<ArrayBufferLike>`, `<SharedArrayBuffer>` | `1 3 3 true` | **`-1 -1 -1 false`** | `1 3 3 true` |
| `Buffer<ArrayBuffer>` (`indexOf("lo")`, `indexOf(108, 3)`, …) | `3 2 3 3 true false` | **`-1 -1 -1 -1 false false`** | `3 2 3 3 true false` |
| `Readonly<Uint8Array>` (never the generic spelling; reaches the Array helpers) | `1 3 3 true` | **`-1 -1 -1 false`** | `1 3 3 true` |

`join` on the same mis-typed receivers printed `5.09279032885e-313-1.27…`
instead of `5-44-3-44-200-1`.

Why `Float32Array<ArrayBuffer>` was never wrong: a `Float32Array` is a
`TypedArrayHeader` in the typed-array registry, which the array search
helpers already re-check at runtime (#3148). A `Uint8Array` is a
`BufferHeader`, registered as a buffer, which they did not.

Two layers, so the wrong static guess can no longer become a wrong value:

- **HIR** (`lower_types/extract.rs`, `lower_types.rs`): a type reference or
  `new X<…>()` whose base is a typed array, `Buffer` or `DataView` erases
  its arguments to `Type::Named(X)` — unless a user class of that name is
  declared in the module (`class Buffer<T>`), which keeps `Generic` so
  monomorphization still finds its specialization. The lowered body of a
  function is now byte-identical under `Uint8Array` and
  `Uint8Array<ArrayBuffer>` for 41 member accesses (asserted in
  `buffer_backed_generic_tests.rs`), so the generic spelling takes exactly
  the tiers the bare name takes.
- **Runtime** (`array/search.rs`, `array/join.rs`): `js_array_indexOf_jsvalue`,
  `js_array_last_index_of_jsvalue`, `js_array_includes_jsvalue` and
  `js_array_join` ask `buffer_receiver_dispatch` above the array-only
  funnel, the same way `forEach`/`map`/`reduce` did in #8137, so a
  Buffer-backed `Uint8Array` that reaches them through a lying type
  (`Readonly<Uint8Array>`, `as unknown as number[]`) is answered by the
  shared uint8 dispatcher rather than by reinterpreting its bytes.

Found by the TypeScript Native Messaging host in
guest271314/NativeMessagingHosts: its `sendMessage(message:
Uint8Array<ArrayBuffer>)` splits replies over 1 MiB at a comma found with
`message.indexOf(COMMA, searchStart)`; with `-1` it never split, and a
1,048,581-byte reply went out as one frame over the host→browser limit.
`test_gap_10894_native_messaging_split.ts` is that loop verbatim at 1 MiB,
1 MiB + 5 B, 2 MiB and 5 MiB; `test_gap_10894_typed_array_generic_annotation.ts`
is the full audit — 14 buffer-backed classes × 3 argument spellings × ~30
members, plus every annotation position (alias, union, optional, array
element, field, return type, rest, destructuring, `Map`/`Record`/tuple
value, `AsyncGenerator<…>`, `Promise<…>`, `new X<…>()`).

Not changed, noted for follow-up: `slice`/`find`/`fill`/… on a receiver
cast `as unknown as number[]` still read the buffer as f64 slots (the rest
of the #8137 family); `Uint8Array.prototype.indexOf(300)` answers `1` on
every path because the Buffer arm masks numeric needles to a byte where
`%TypedArray%` uses strict equality; and `v instanceof DataView` is
`false` for every receiver.
