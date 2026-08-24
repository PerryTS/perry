# Native Layout Values

Perry keeps ordinary TypeScript values ordinary. A `number` still has
JavaScript number semantics, and normal objects and arrays remain managed
values. At boundaries where byte width and C-compatible layout are part of
correctness, `perry/native` provides an explicit, opt-in contract.

```typescript,no-test
import {
  i8,
  i16,
  u8,
  u16,
  i32,
  u32,
  u64,
  f32,
  isize,
  type pod,
  type PodView,
  NativeArena,
  sizeof,
  alignof,
  offsetof,
} from "perry/native";

const opcode = u8(inputOpcode);
const delta = i8(inputDelta);
const port = u16(inputPort);
const flags = u32(inputFlags);
const sequence = u64(inputSequence);
const gain = f32(inputGain);

type PacketHeader = pod<{
  flags: u32;
  sequence: u64;
  gain: f32;
}>;

const header: PacketHeader = {
  flags: u32(inputFlags),
  sequence: u64(inputSequence),
  gain: f32(inputGain),
};

const byteLength = sizeof<PacketHeader>();
const alignment = alignof<PacketHeader>();
const sequenceOffset = offsetof<PacketHeader>("sequence");

const arena = NativeArena.alloc(byteLength * 16);
const headers: PodView<PacketHeader> = arena.podView(0, 16);

console.log(byteLength, alignment, sequenceOffset, headers.length);
arena.dispose();
```

## Supported scalar layouts

The public profile exposes the native representations the POD and native ABI
verifier supports:

| Type | Native representation |
|---|---|
| `i8` | signed 8-bit integer |
| `i16` | signed 16-bit integer |
| `u8`, `byte` | unsigned 8-bit integer (`byte` is a type alias) |
| `u16` | unsigned 16-bit integer |
| `i32` | signed 32-bit integer |
| `i64` | signed 64-bit integer |
| `u32` | unsigned 32-bit integer |
| `u64` | unsigned 64-bit integer |
| `usize` | target pointer-sized unsigned integer |
| `isize` | target pointer-sized signed integer |
| `f32` | IEEE-754 binary32 |
| `f64` | IEEE-754 binary64 |

These names replace the internal-looking `PerryI32`, `PerryU64`,
`PerryF32`, and related spellings in new application code. The old names
remain available as compatibility aliases.

Each scalar name is both a type and a checked conversion function. Integer
conversions accept only finite integral values in range; unsigned conversions
also reject negative values. Because standalone results remain
JavaScript-compatible numbers, `i64`, `u64`, `isize`, and `usize` reject values outside
the safe-integer range rather than returning an imprecise number. `f32` makes
binary32 rounding explicit and rejects values that are non-finite before or
after rounding; `f64` validates that its input is finite. A non-number throws a
`TypeError`; an unrepresentable number throws a `RangeError`.

```typescript,no-test
import { i8, i16, u8, u16, i32, u32, u64, isize, f32 } from "perry/native";

const delta = i8(dynamicDelta);
const offset16 = i16(dynamicOffset);
const opcode = u8(dynamicOpcode);
const port = u16(dynamicPort);
const offset = i32(dynamicOffset);
const count = u32(dynamicCount);
const sequence = u64(dynamicSequence);
const pointerDelta = isize(dynamicPointerDelta);
const ratio = f32(computation);
```

The scalar aliases establish representation inside a `pod` layout and at
supported native ABI boundaries. A matching checked conversion may initialize
a POD field from a dynamic value without forcing the whole record back to an
ordinary object; the conversion guard runs before the value enters the native
record. They do not change standalone TypeScript arithmetic: operators still
follow ordinary JavaScript number rules unless an explicit checked conversion
is used. The brand records intent for POD layout and native boundaries; it is
not a second runtime number object.

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

POD assignment has value semantics. `const copy = header` snapshots the
declared scalar fields into independent storage, so later property writes do
not alias the original. Passing a standalone POD to an ordinary function also
passes an independent value, even when the compiler would otherwise inline
that function. Nested object initializers are flattened recursively according
to the declared layout. `PodView<T>` is different: it is an explicit view over
arena storage and aliases that storage by design.

## Materialization and optimization guarantees

The checked value and layout behavior above is stable language contract. The
compiler may keep a proven POD local or scalar in native storage, but that is
an optimization rather than an observable promise. Passing values through an
ordinary TypeScript function, array, object, or other managed API may
materialize JavaScript-compatible numbers or objects. Materialization must
preserve the checked value; in particular, no `i64`, `u64`, `isize`, or
`usize` conversion can silently introduce an imprecise JavaScript number.

At a `perry.nativeLibrary` boundary, manifest descriptors restore the exact C
ABI width and signedness. A manifest POD may reference the exported TypeScript
`pod<T>` declaration; compilation and `perry native validate` reject drift in
field type or order before generating a call. See [Native Library Manifest
v1](../native-libraries/manifest-v1.md#functions).

## Arena ownership

`NativeArena.alloc` owns a fixed native allocation. `view` creates a typed
array view and `podView` creates a `PodView<T>` over that allocation. Byte
offsets, lengths, alignment, and disposal are checked by the existing native
memory verifier and runtime guards.

Call `dispose()` when the allocation is no longer needed. Access through a
view after disposal is an error. `PodView` is currently exposed as read-only;
mutable borrowed POD views are not yet part of the public contract.
