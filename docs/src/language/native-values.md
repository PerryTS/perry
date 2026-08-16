# Native Layout Values

Perry keeps ordinary TypeScript values ordinary. A `number` still has
JavaScript number semantics, and normal objects and arrays remain managed
values. At boundaries where byte width and C-compatible layout are part of
correctness, `perry/native` provides an explicit, opt-in contract.

```typescript
import {
  type u32,
  type u64,
  type f32,
  type pod,
  type PodView,
  NativeArena,
  sizeof,
  alignof,
  offsetof,
} from "perry/native";

type PacketHeader = pod<{
  flags: u32;
  sequence: u64;
  gain: f32;
}>;

const byteLength = sizeof<PacketHeader>();
const alignment = alignof<PacketHeader>();
const sequenceOffset = offsetof<PacketHeader>("sequence");

const arena = NativeArena.alloc(byteLength * 16);
const headers: PodView<PacketHeader> = arena.podView(0, 16);

console.log(byteLength, alignment, sequenceOffset, headers.length);
arena.dispose();
```

## Supported scalar layouts

The first public slice exposes the native representations the POD and native
ABI verifier already supports:

| Type | Native representation |
|---|---|
| `i32` | signed 32-bit integer |
| `i64` | signed 64-bit integer |
| `u32` | unsigned 32-bit integer |
| `u64` | unsigned 64-bit integer |
| `usize` | target pointer-sized unsigned integer |
| `f32` | IEEE-754 binary32 |
| `f64` | IEEE-754 binary64 |

These names replace the internal-looking `PerryI32`, `PerryU64`,
`PerryF32`, and related spellings in new application code. The old names
remain available as compatibility aliases.

The scalar aliases currently establish representation inside a `pod` layout
and at supported native ABI boundaries. They do not change the semantics of
standalone TypeScript arithmetic. Checked scalar conversion functions,
additional widths (`i8`, `i16`, `u8`, `u16`, and `isize`), and guaranteed
native lanes across general-purpose collections are later parts of the native
value profile.

## POD records

`pod<T>` asks the compiler to verify a C-layout record. The supported field
set is deliberately narrow:

- the scalar aliases listed above;
- ordinary `number`, represented as `f64`;
- nested `pod` records; and
- compatible legacy `Perry*` native scalar markers.

Managed or pointer-bearing values such as `string`, normal arrays, class
instances, closures, promises, maps, and sets are rejected as POD fields.
Field order is source order. Perry computes target C alignment and padding;
`sizeof`, `alignof`, and `offsetof` become compile-time constants and require
an explicit POD type argument. `offsetof` also requires a string-literal field
path, with dotted paths accepted for nested records.

POD layout uses the target's native byte order. It does not define a portable
serialization format; use `DataView` or another explicit encoder when stored
or transmitted bytes require a specified endianness.

## Arena ownership

`NativeArena.alloc` owns a fixed native allocation. `view` creates a typed
array view and `podView` creates a `PodView<T>` over that allocation. Byte
offsets, lengths, alignment, and disposal are checked by the existing native
memory verifier and runtime guards.

Call `dispose()` when the allocation is no longer needed. Access through a
view after disposal is an error. `PodView` is currently exposed as read-only;
mutable borrowed POD views are not yet part of the public contract.
