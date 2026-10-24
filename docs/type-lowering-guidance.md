Perry’s next major performance step should be **representation-aware type lowering**: keep values in native typed form for as long as possible, and box into `JSValue` only at true dynamic boundaries.

Right now Perry already has the ingredients: HIR type inference, monomorphized generics, `Int32` fast paths, array bounds elimination, numeric field recognition, scalar replacement, NaN-boxed `JSValue`, shape caching, fixed class IDs, and runtime-specialized support for strings, arrays, maps, buffers, dates, BigInts, regexes, promises, and async state machines. The problem is that too much of the system still uses the universal `JSValue` representation too early. The fastest Perry should look less like “native code calling a JS runtime helper often” and more like “a static compiler that only falls back to JS dynamic semantics where the program actually needs them.”

## Main guidance

The core rule should be:

```text
Do not lower TypeScript values directly to JSValue.
Lower them to typed SSA values first.
Box only at dynamic boundaries.
```

A good target model is:

```text
TypeScript/HIR type
  → Perry type facts
  → representation-specific IR
  → LLVM native value
  → JSValue only if needed
```

For example:

```text
number                 → f64
integer-stable number  → i32 / u32 / i53
boolean                → i1
string                 → PerryStringRef, not raw JSValue
class Point            → ptr PointObjectLayout
number[] packed        → ptr ArrayF64
any / unknown          → i64 JSValueBits
JS interop handle      → JSHandleValue
```

This matters because LLVM can optimize `i32`, `double`, `ptr`, and typed loads. It cannot reason well about every value being a NaN-boxed `f64`. Perry’s current architecture says every JS value crossing a function boundary is a NaN-boxed `f64`, with tags for strings, objects, int32, BigInt, short strings, handles, and singleton values. That is fine as a **public ABI**, but it should not be the default internal representation inside optimized functions.

## Use `JSValue` as an ABI, not as the optimizer’s native type

Keep `JSValue` for:

```text
public function boundaries
unknown calls
any / unknown
dynamic property access
generic arrays
object dictionaries
exceptions
closures that escape
Promise/microtask storage
thread serialization
V8/QuickJS bridge values
```

But inside a compiled function, Perry should prefer typed values.

Example target shape:

```text
// Public generic trampoline.
foo$jsvalue(JSValue a, JSValue b) -> JSValue {
  if a,b are numbers:
    return box_number(foo$number_number(a as f64, b as f64))
  else:
    return foo$generic(a, b)
}

// Internal typed clone.
foo$number_number(f64 a, f64 b) -> f64 {
  return a + b
}
```

The same applies to classes:

```text
Point.distance$typed(ptr Point, ptr Point) -> f64
Point.distance$generic(JSValue this, JSValue other) -> JSValue
```

This gives Perry a static-compiler version of what a JIT does dynamically: one generic path for correctness, and typed paths for speed.

## Represent `JSValue` bits as `i64` in LLVM IR

Even if the external ABI still passes `JSValue` as `f64`, Perry should seriously consider representing boxed values internally as `i64` bit patterns, not as LLVM `double`.

NaN-boxing depends on preserving payload bits exactly. But LLVM and CPU floating-point optimizations naturally treat `double` as a numeric value, not as a tagged pointer carrier. Perry’s own `JSValue::is_number` logic distinguishes real IEEE numbers from Perry-owned positive quiet-NaN tag bands, and the runtime already has careful handling for tags such as `SHORT_STRING_TAG`, `STRING_TAG`, and `POINTER_TAG`.

Recommended internal split:

```text
NumberValue  = double
BoxedValue   = i64
PointerValue = ptr
BoolValue    = i1
Int32Value   = i32
Uint32Value  = i32 with unsigned interpretation
```

Only bitcast between `i64` and `double` at ABI edges where the current ABI requires `f64`.

This also avoids accidental optimizer corruption from fast-math flags. JavaScript numeric semantics include `NaN`, infinities, and signed zero, so broad fast-math should not be applied to general JS `number` operations unless Perry has proven the operation is in a restricted numeric domain.

## Build a richer type-fact lattice

Current HIR types are useful but too coarse. `Type::Number`, `Type::String`, `Type::Array(elem)`, `Type::Named(name)`, and `Type::Any` are not enough for aggressive lowering. Perry should keep HIR types, then add a second layer of **type facts**.

Recommended fact shape:

```text
Value facts:
  kind: number | int32 | uint32 | int53 | bool | string | object | array | bigint | any
  nullability: non-null | nullable | nullish | unknown
  representation: unboxed | boxed | pointer | handle
  range: integer bounds if known
  constant: literal value if known

Array facts:
  element kind: f64 | i32 | uint32 | JSValue | string | object
  packed vs holey
  length stable inside region
  capacity stable inside region
  no external alias
  no body write can grow/shrink

Object facts:
  exact class id
  exact shape id
  field layout
  field pointer bitmap
  frozen/sealed/no-extend state
  dictionary fallback possible or impossible

Effect facts:
  may allocate
  may call unknown code
  may mutate array length
  may mutate object shape
  may throw
  may access JS bridge
  may run microtasks
```

This turns Perry’s optimizer from “infer a type once” into “carry a proof.” That proof then decides whether Perry emits raw native loads, bounds checks, dynamic dispatch, write barriers, or generic helper calls.

## Generalize the existing integer fast path

Perry already has a real performance win here. It recognizes integer-valued expressions, tracks `i32` loop-counter slots, uses integer modulo instead of `fmod`, and avoids repeated `fptosi/sitofp` round-trips in hot array-walking loops. The provided benchmark methodology says this is why integer modulo can become a single integer instruction instead of a libm `fmod` call, and why array read/write loops can become raw `getelementptr + load` patterns that LLVM can vectorize.

The next step is to expand the numeric lattice:

```text
i32       signed ToInt32 domain
u32       unsigned ToUint32 domain
i53       safe JS integer domain
f64       general JS number
f64-nonNaN proven non-NaN
f64-finite proven finite
```

Do not force everything integer-like into `i32`. JavaScript has several subtly different integer domains:

```text
x | 0      → signed i32
x >>> 0    → unsigned u32
array idx  → uint32-ish but constrained by length
number     → f64, often integer-valued but not always safely i32
```

Perry’s comments already show why this matters: previous `i32` shadowing could silently corrupt accumulators that were integer-stable but not actually safe as signed 32-bit values. The current gating via index-used locals, strictly bounded locals, and unsigned locals is the right direction. Extend that into a first-class numeric domain system rather than a set of local special cases.

## Make arrays representation-specialized

Arrays should not all be `ArrayHeader + JSValue[]`.

Recommended array kinds:

```text
PackedF64Array
PackedI32Array
PackedU32Array
PackedStringArray
PackedObjectArray<class_id>
PackedValueArray
HoleyValueArray
DictionaryArray
TypedArray-backed variants
```

For `number[]` in a proven packed numeric loop, Perry should lower:

```ts
for (let i = 0; i < arr.length; i++) {
  sum += arr[i]
}
```

to roughly:

```text
arr_ptr = checked_unbox_packed_f64_array(arr)
len = arr.length
data = arr.data

for i32 i in [0, len):
  sum = fadd sum, load data[i]
```

No per-iteration NaN-box decode. No per-iteration length load. No bounds check if the loop proof establishes `i < arr.length`. No runtime helper call. Perry already has a narrower version of this with cached lengths, `bounded_index_pairs`, and `i32_counter_slots`; generalize it to element-kind-specialized arrays.

For stores, use transitions:

```text
PackedF64Array + number store      → stay PackedF64Array
PackedF64Array + undefined store   → transition to PackedValueArray or HoleyValueArray
PackedI32Array + f64 store         → transition to PackedF64Array or PackedValueArray
PackedObjectArray<C> + object D    → stay only if D <= C-compatible
```

This is where Perry can exploit its AOT restrictions. Since Perry does not support general `eval`, `new Function`, dynamic import, dynamic `require`, full prototype mutation, or full Proxy trapping, it has fewer invalidation hazards than V8. Those limitations are not just compatibility gaps; they are optimization permissions.

## Add array-loop versioning

For uncertain arrays, compile two paths:

```text
fast path:
  guard array is PackedF64Array
  guard no holes
  guard length stable
  run typed loop

slow path:
  generic JS array access
```

Example:

```text
if likely(is_packed_f64_array(arr)) {
  return sum_packed_f64(arr)
}
return sum_generic_jsvalue(arr)
```

This is AOT-friendly. It does not require a JIT. It only requires small guarded clones.

Use this for:

```text
Array.prototype.map/filter/reduce
for loops over arr.length
JSON parse/stringify internal loops
Buffer/Uint8Array loops
string scanning
numeric kernels
```

Code size must be controlled. Do not clone every function for every type combination. Clone only when:

```text
function is hot by benchmark/profile
loop body is small
specialization removes helper calls
array kind is stable
generic fallback remains available
```

PGO is a good fit here because it lets the compiler choose which clones matter for real workloads. LLVM’s own documentation describes PGO as a way for a compiler to optimize according to how code actually runs, with representative profile selection being important. ([LLVM][1])

## Make object/class fields unboxed where possible

Perry’s object model currently uses `ObjectHeader`, `class_id`, `field_count`, `keys_array`, inline property slots, shape caching, `KEYS_INDEX`, overflow fields, and vtable-based dynamic dispatch. That is a workable JS object model, but class instances with declared fields should not be treated like generic dictionaries in hot paths.

For classes, generate fixed layouts:

```ts
class Point {
  x: number
  y: number
}
```

Target layout:

```text
ObjectHeader
class_id
shape_id
field_bitmap
x: f64
y: f64
```

Not:

```text
ObjectHeader
keys_array = ["x", "y"]
slots[0] = JSValue(number)
slots[1] = JSValue(number)
```

The second layout is more dynamic, but much slower. The first layout gives LLVM ordinary typed loads:

```llvm
%x_ptr = getelementptr %Point, ptr %p, field_x
%x = load double, ptr %x_ptr
```

For nullable/dynamic fields, use mixed layout:

```text
f64 fields
i32 fields
pointer fields
JSValue spill fields
overflow dictionary
```

This also improves GC. A typed field layout gives the collector a pointer bitmap, so it scans only pointer fields instead of inspecting every slot dynamically.

## Fix scalar replacement across method calls

The current scalar replacement limitation is important: Perry can stack-allocate or decompose non-escaping object literals only when the object is accessed exclusively through field get/set; any method call defeats it.

That should be one of the highest-priority compiler improvements.

Example:

```ts
class Point {
  constructor(public x: number, public y: number) {}
  sum() { return this.x + this.y }
}

let p = new Point(x, y)
total += p.sum()
```

Current likely behavior:

```text
allocate Point
store x/y
dynamic or semi-dynamic method call
load fields
GC-visible object
```

Target behavior:

```text
p.x = scalar x
p.y = scalar y
inline Point.sum
total += x + y
no allocation
```

To get there, Perry needs method summaries:

```text
method Point.sum:
  receiver escapes? no
  mutates receiver shape? no
  reads fields: x, y
  writes fields: none
  may call unknown? no
  may throw? no
```

Then escape analysis can treat simple method calls as field operations. LLVM’s SROA pass is specifically designed to break analyzable aggregate allocas into scalar SSA values, and its vectorizers can then operate on clean scalar/loop IR; Perry should feed LLVM IR that makes those passes obvious instead of hiding work behind runtime helper calls. ([LLVM][2]) ([LLVM][3])

## Replace stringly dynamic dispatch with IDs

`js_native_call_method` currently receives a method name pointer and length, builds/uses a string name, handles JS handles, class static methods, vtable lookup, prototype objects, and fallback paths. That is correct but expensive for hot calls.

For compiled code, method/property names should be lowered to interned IDs at compile time:

```text
"toString"  → SymbolId / PropertyId 17
"value"     → FieldId 3
"length"    → BuiltinPropertyId::Length
```

Then dispatch can be:

```text
if exact class id known:
  direct call function pointer

else if class id known but subclass possible:
  vtable[class_id][method_id]

else:
  generic js_native_call_method_by_id

only final fallback:
  js_native_call_method_by_string
```

Hot-path method calls should not allocate Rust `String`s, hash method names, or scan strings. For static class methods, the same ID system should apply.

## Unify string lowering; eliminate the SSO footgun

The current short-string optimization is valuable, but the strict `is_string()` versus `is_any_string()` distinction is a correctness and performance hazard. The context explicitly warns that `is_string()` only recognizes heap `STRING_TAG`, while `SHORT_STRING_TAG` can fall into wrong branches and even be dereferenced as a pointer if call sites are not careful.

Recommended fix:

```text
PerryStringRef:
  Short { bytes[5], len }
  Heap { ptr: *StringHeader }
```

Then generated code and runtime helpers should use one abstraction:

```text
is_string_like(value)
string_len(value)
string_bytes(value)
string_materialize_if_needed(value)
```

Rename low-level predicates to make misuse hard:

```text
is_heap_string()
is_short_string()
is_any_string()
```

Do not allow new runtime code to branch on `is_string()` unless the name means “any string.” For performance, specialize string operations:

```text
short + short       → inline small concat if result <= 5
heap refcount == 1 → append in place
concat chain        → one allocation
string scan         → SIMD path
property key        → interned ID / pointer identity
```

The provided context already describes SSO, in-place append, concat-chain optimization, and SIMD string scanning. The main improvement is making the type lowering and runtime API impossible to misuse.

## Use Perry’s unsupported JS features as optimization assumptions

Perry does not support or only partially supports several highly dynamic JS features: full `Proxy`, full `Reflect`, `eval`, `new Function`, dynamic import, user-space dynamic `require`, full prototype mutation, `SharedArrayBuffer`, and `Atomics`.

That means Perry can assume much more than V8 in native-compiled mode:

```text
class layouts do not get monkey-patched at runtime
prototype methods do not arbitrarily change
static ESM imports form a closed module graph
no eval can introduce new code
no SharedArrayBuffer means no cross-thread mutation races
deep-copy threading means local arrays are not concurrently modified
```

Perry should formalize this into compilation modes:

```text
strict-native mode:
  assumes Perry limitations
  strongest type lowering
  no dynamic fallback except explicit JS runtime bridge

compat mode:
  more JSValue paths
  more guards
  less layout specialization
```

This lets Perry turn compatibility limitations into performance wins without pretending to be fully dynamic JavaScript.

## Add effect analysis before lowering

Many current optimizations depend on proving that a loop body does not mutate `arr.length`, does not reassign the loop counter, and does not invalidate the cached length. Perry already does some of this for bounded index pairs.

Make this a general effect system:

```text
Effect::ReadsArrayLength(arr)
Effect::WritesArrayLength(arr)
Effect::WritesArrayElement(arr)
Effect::MayGrowArray(arr)
Effect::MutatesShape(obj)
Effect::CallsUnknown
Effect::Allocates
Effect::MayThrow
Effect::RunsMicrotasks
Effect::TouchesJSHandle
```

Then lowering decisions become principled:

```text
Can cache arr.length?
  yes if no effect in loop may write length or reassign arr

Can eliminate bounds check?
  yes if loop induction range is within cached length

Can direct-load field?
  yes if receiver shape is stable and no effect mutates it

Can stack-allocate object?
  yes if object does not escape through call, closure, return, throw, async, or unknown store

Can skip write barrier?
  yes if stored value is statically primitive or parent is young
```

This will improve both performance and correctness because optimizations become proof-based rather than pattern-based.

## Emit better LLVM metadata

Perry should make LLVM’s optimizer see what Perry already knows.

For typed arrays and fixed class layouts, emit:

```text
nonnull
dereferenceable(N)
align
noalias for fresh allocations
alias.scope / noalias for independent arrays
TBAA metadata for headers, lengths, capacities, fields, data buffers
range metadata for lengths, tags, enum values
cold/noinline for generic fallbacks
alwaysinline for tiny tag checks and unbox helpers
readonly/readnone/willreturn/nounwind where valid
```

LLVM’s LangRef documents `dereferenceable`, `noalias`, `alias.scope`, `TBAA`, and `range` metadata; these are exactly the kinds of facts Perry can provide from its type/layout system. ([LLVM][4]) ([LLVM][4]) ([LLVM][4]) ([LLVM][4])

The important point: do not merely generate “correct” LLVM IR. Generate IR that exposes aliasing, bounds, layout, and type facts.

## Lower write barriers statically

Write barriers should not be emitted as generic runtime calls for every store.

For every store site, codegen should decide:

```text
storing number/bool/null/undefined/int32?
  no pointer child → no barrier

parent proven young?
  no old→young edge → no barrier

child proven old or non-GC?
  no young child → no barrier

parent old and child maybe young?
  inline fast card barrier
```

The GC context says Perry’s current barrier fires on every heap store emitted by codegen, decodes parent and child, checks old→young, and dirties a page when needed. That is semantically right, but it leaves too much work for runtime.

Type lowering can remove many barriers before codegen:

```text
obj.x = 3.14        // no barrier
obj.flag = true     // no barrier
obj.count = i32     // no barrier
youngObj.child = y  // no old→young barrier
oldObj.child = y    // inline card barrier only if y may be young pointer
```

This also argues for unboxed class fields. A `number` field stored as raw `f64` never needs a GC barrier.

## Improve Map/Set lowering with key-specialized tables

The current runtime has separate side-table indices for numeric keys, string keys, and sets, with content hashing for strings.

The compiler should exploit that:

```ts
const m = new Map<string, number>()
m.set(k, v)
m.get(k)
```

should lower to:

```text
MapStringNumber
key: PerryStringRef / interned string id where possible
value: f64
```

Not:

```text
generic Map<JSValue, JSValue>
generic hash
generic equality
boxed value
```

Recommended specializations:

```text
Map<string, T>
Map<number, T>
Map<int32, T>
Map<object identity, T>
Set<string>
Set<int32>
```

For `Record<string, V>` and object dictionaries, use the same idea: once an object crosses the `KEYS_INDEX_THRESHOLD`, compile dynamic key operations against a dictionary representation directly rather than repeatedly going through generic object field helpers.

## Rework BigInt representation

The current BigInt design uses fixed 1024-bit storage, mainly to satisfy crypto workloads where secp256k1 intermediates can exceed 512 bits.

That is good for crypto kernels, but it is too heavy as the default BigInt representation.

Recommended split:

```text
SmallBigInt:
  inline i64/u64 or two limbs

MediumBigInt:
  heap variable-limb Vec<u64>

CryptoBigInt1024:
  fixed 16-limb path for known crypto packages / specialized kernels
```

Then lowering can choose:

```text
1n + 2n                    → SmallBigInt or constant fold
BigInt loop counter         → SmallBigInt
crypto modular multiply     → CryptoBigInt1024
unknown BigInt expression   → generic variable-limb BigInt
```

This prevents every BigInt from paying crypto-sized costs.

## Treat async lowering as allocation lowering

Perry already lowers async/await into generator/state-machine form, runs promise microtasks, and has optimized the microtask loop by using one outer `setjmp` instead of one per microtask. The async bridge also avoids creating `JSValue` objects on Tokio worker threads and defers conversion to the main thread because arenas are thread-local.

The next performance step is to make async lowering typed:

```text
async function f(): Promise<number>
```

should become:

```text
state machine result slot: f64
Promise<number> continuation: typed f64 until boxed
```

Avoid:

```text
state machine slot: JSValue
every await result: boxed
every continuation: generic closure
```

Recommended async optimizations:

```text
typed result slots in state machines
typed capture slots
static continuation structs instead of generic closures
Promise allocation reuse for await chains
no boxing until resolving externally-visible Promise
microtask queue entries specialized by callback signature
```

The existing `MT_STEP_CHAIN_REUSE_HIT` style optimization should be expanded into a general “typed async continuation” path.

## Use internal typed calling conventions

Perry’s monomorphization is already a strong asset. Generics are specialized into mangled function/class names such as `identity$number`.

Extend that idea beyond TypeScript generics:

```text
Function clone dimensions:
  argument representation
  return representation
  receiver class id
  array element kind
  nullability
  closure capture layout
```

Example:

```text
sum$ArrayF64__f64
sum$ArrayI32__i32
sum$ArrayValue__JSValue
sum$generic
```

But control code size:

```text
clone only loops/functions above threshold
clone only if helper calls disappear
clone only if call graph is stable
merge clones with same machine representation
cap clone count per function
use profile data to choose
```

This gives Perry a static analog to V8’s specialization without needing a JIT.

## Add package-level specialization

Perry can know the whole package at compile time. Use that.

For npm/native packages that Perry supports directly, ship lowering profiles:

```text
fastify:
  object-shape stable request/response paths
  string/header maps
  async Promise chains

mysql2:
  row object shapes
  date/string/buffer decoding
  typed column arrays

redis:
  string/buffer heavy paths
  command array flattening

noble/ethers:
  BigInt/Uint8Array crypto kernels
  fixed-limb arithmetic
```

This should not be handwritten one-off hacks in codegen. It should be a declarative profile system:

```text
known stable shapes
known method purity
known allocation patterns
known typed arrays
known no-dynamic-require subset
```

Then the normal optimizer consumes those facts.

## Make lowering observable

Before adding many optimizations, add a compiler report:

```text
perry build --explain-lowering
```

For each function:

```text
boxes inserted: 42
unboxes inserted: 17
js_number_coerce calls: 3
runtime property gets: 8
direct field loads: 21
bounds checks eliminated: 14
barriers eliminated: 32
object allocations scalar-replaced: 6
array kind: PackedF64Array
generic fallback emitted: yes
reason scalar replacement failed: method call escapes receiver
reason bounds check kept: loop body may mutate length
reason typed call failed: callee return type unknown
```

This will pay for itself quickly. Perry’s biggest optimization risk is silent missed lowering: code still works, but one helper call in a hot loop destroys performance.

## Recommended implementation order

First, split internal `JSValue` representation into `i64 JSValueBits` and typed native values. Keep the existing external ABI if needed, but stop letting LLVM see boxed values as ordinary floating-point values unless they are actually numbers.

Second, add a `TypeFacts` pass after HIR lowering and before LLVM generation. This should compute numeric domains, array kinds, object shapes, nullability, escape state, and side effects.

Third, implement late boxing. Function bodies should use native typed values; boxes should be inserted only at returns, unknown calls, dynamic stores, closure captures, async suspension, thread serialization, and JS interop.

Fourth, create internal typed function clones plus generic trampolines. Start with `number`, `int32`, `boolean`, `string`, and packed numeric arrays.

Fifth, generalize array lowering into packed/holey/value/dictionary representations. Make the existing bounded-index and cached-length logic a special case of a broader array-fact system.

Sixth, make fixed class layouts use unboxed fields and direct method calls when the receiver class is exact. Add method purity/effect summaries so scalar replacement works across simple method calls.

Seventh, replace string-name dispatch with interned property/method IDs. Keep string fallback for dynamic cases only.

Eighth, unify string handling around `PerryStringRef` so SSO and heap strings share one safe lowering path.

Ninth, specialize Map/Set/Record by key and value kind.

Tenth, apply PGO to choose which typed clones to emit and which generic paths to mark cold.

## What not to do

Do not rely on TypeScript annotations as runtime truth. TypeScript types guide optimization, but values can still enter through `any`, unknown package boundaries, dynamic data, JSON, V8/QuickJS handles, or native callbacks. Use annotations as optimistic facts only when the compiler can prove the boundary is closed, or emit guards.

Do not globally enable fast-math. JavaScript `number` semantics are not C `-ffast-math` semantics. Signed zero, `NaN`, infinities, and coercions matter.

Do not solve performance primarily by adding more runtime helpers. The goal is fewer helper calls in hot paths.

Do not let monomorphization explode code size. Specialization should be costed.

Do not keep expanding object side tables for hot class instances. Use side tables for dynamic dictionaries; use fixed typed layouts for classes and stable shapes.

## Bottom line

The right direction for Perry is:

```text
AOT TypeScript compiler
  + typed SSA
  + late boxing
  + guarded typed clones
  + packed array/object layouts
  + effect/range/escape analysis
  + LLVM metadata
  + generic JSValue fallback
```

Perry should not try to beat V8 by making a faster generic JS object model. It should beat V8 where AOT has an advantage: closed-world TypeScript, fixed class layouts, static imports, typed arrays, monomorphized functions, predictable async state machines, and compiler-proven loops.

The best single sentence guidance is: **make `JSValue` the fallback representation, not the default representation.**

[1]: https://llvm.org/docs/HowToBuildWithPGO.html "How To Build Clang and LLVM with Profile-Guided Optimizations — LLVM 23.0.0git documentation"
[2]: https://llvm.org/docs/Passes.html?utm_source=chatgpt.com "LLVM's Analysis and Transform Passes"
[3]: https://llvm.org/docs/Vectorizers.html?utm_source=chatgpt.com "Auto-Vectorization in LLVM — LLVM 23.0.0git documentation"
[4]: https://llvm.org/docs/LangRef.html "LLVM Language Reference Manual — LLVM 23.0.0git documentation"
