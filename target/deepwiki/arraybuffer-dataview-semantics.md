# DeepWiki Response

Repository: `boa-dev/boa`
Question: For implementing ECMAScript ordinary ArrayBuffer and DataView semantics, summarize the ArrayBuffer constructor/prototype/static surface, ArrayBuffer.isView behavior, DataView internal slots and subclass recognition, ArrayBuffer.prototype.byteLength getter descriptors and brand checks, ArrayBuffer.prototype.slice bounds/defaults, and where to avoid scope creep into SharedArrayBuffer, resizable/growable buffers, transfer semantics, or TypedArray transform methods. Focus on implementation-relevant details and tests.
Search: https://deepwiki.com/search/for-implementing-ecmascript-or_a7d60b37-9322-46df-a259-cc0dbd9f8cd4

**Question**: For implementing ECMAScript ordinary ArrayBuffer and DataView semantics, summarize the ArrayBuffer constructor/prototype/static surface, ArrayBuffer.isView behavior, DataView internal slots and subclass recognition, ArrayBuffer.prototype.byteLength getter descriptors and brand checks, ArrayBuffer.prototype.slice bounds/defaults, and where to avoid scope creep into SharedArrayBuffer, resizable/growable buffers, transfer semantics, or TypedArray transform methods. Focus on implementation-relevant details and tests.

This response summarizes the implementation of ECMAScript `ArrayBuffer` and `DataView` semantics in the Boa engine, focusing on the constructor, prototype, static methods, internal slots, and brand checks, while also highlighting areas to avoid scope creep.

## ArrayBuffer Constructor, Prototype, and Static Surface

The `ArrayBuffer` object in Boa is represented by the `ArrayBuffer` struct [1](#ref-q1-1) . Its constructor, `ArrayBuffer::constructor`, handles the creation of new `ArrayBuffer` instances [2](#ref-q1-2) . It takes a `byteLength` and an optional `maxByteLength` to allocate the buffer [3](#ref-q1-3) . The `ArrayBuffer::allocate` function performs the actual memory allocation and initialization of internal slots like `[[ArrayBufferData]]`, `[[ArrayBufferByteLength]]`, and `[[ArrayBufferMaxByteLength]]` [4](#ref-q1-4) .

The `ArrayBuffer` prototype defines several accessors and methods:
*   `ArrayBuffer.prototype.byteLength`: A getter that returns the length of the `ArrayBuffer` in bytes [5](#ref-q1-5) .
*   `ArrayBuffer.prototype.resizable`: A getter that indicates if the `ArrayBuffer` can be resized [6](#ref-q1-6) .
*   `ArrayBuffer.prototype.maxByteLength`: A getter that returns the maximum byte length of a resizable `ArrayBuffer` [7](#ref-q1-7) .
*   `ArrayBuffer.prototype.slice`: Creates a new `ArrayBuffer` that contains a copy of the bytes from the original `ArrayBuffer` within the specified range [8](#ref-q1-8) .
*   `ArrayBuffer[@@species]`: A static accessor that returns the `this` value, allowing subclasses to define their own constructor for `slice` operations [9](#ref-q1-9) .

## ArrayBuffer.isView Behavior

The static method `ArrayBuffer.isView` determines if an argument is an `ArrayBuffer` view (either a `TypedArray` or a `DataView`) [10](#ref-q1-10) . It checks if the argument is an object and if it has a `[[ViewedArrayBuffer]]` internal slot, which is present in both `TypedArray` and `DataView` objects [11](#ref-q1-11) .

## DataView Internal Slots and Subclass Recognition

The `DataView` object is represented by the `DataView` struct [12](#ref-q1-12) . It has the following internal slots [13](#ref-q1-13) :
*   `[[ViewedArrayBuffer]]`: A reference to the underlying `ArrayBuffer` or `SharedArrayBuffer` [14](#ref-q1-14) .
*   `[[ByteLength]]`: The length of the `DataView` in bytes [15](#ref-q1-15) .
*   `[[ByteOffset]]`: The offset in bytes from the start of the `[[ViewedArrayBuffer]]` [16](#ref-q1-16) .

The `DataView` constructor, `DataView::constructor`, validates the `buffer`, `byteOffset`, and `byteLength` arguments and initializes these internal slots [17](#ref-q1-17) . Subclass recognition is handled by `OrdinaryCreateFromConstructor`, which uses the `new_target` to determine the prototype chain [18](#ref-q1-18) .

## ArrayBuffer.prototype.byteLength Getter Descriptors and Brand Checks

The `ArrayBuffer.prototype.byteLength` getter performs a brand check to ensure that `this` value is an `ArrayBuffer` instance [5](#ref-q1-5) . If the `this` value is not an `ArrayBuffer` object, it throws a `TypeError` [19](#ref-q1-19) . It then checks if the buffer is detached; if so, it returns `+0𝔽` [20](#ref-q1-20) . The property is configured with `Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE` [21](#ref-q1-21) .

## ArrayBuffer.prototype.slice Bounds and Defaults

The `ArrayBuffer.prototype.slice` method extracts a portion of the `ArrayBuffer` [22](#ref-q1-22) . It performs several checks:
*   **Brand Check**: Ensures `this` is an `ArrayBuffer` [23](#ref-q1-23) .
*   **Detached Buffer Check**: Throws a `TypeError` if the buffer is detached [24](#ref-q1-24) .
*   **Bounds Calculation**: `start` and `end` arguments are converted to `relativeStart` and `relativeEnd` using `ToIndex` and clamped to the buffer's length [25](#ref-q1-25) . If `end` is `undefined`, it defaults to the buffer's length [26](#ref-q1-26) .
*   **Species Constructor**: Uses `SpeciesConstructor` to create a new `ArrayBuffer` instance, allowing subclasses to control the type of the returned buffer [27](#ref-q1-27) .
*   **Copying Data**: The relevant bytes are copied from the original buffer to the new buffer [28](#ref-q1-28) .

## Avoiding Scope Creep

The implementation explicitly avoids scope creep into `SharedArrayBuffer`, resizable/growable buffers, transfer semantics, or `TypedArray` transform methods by:
*   **SharedArrayBuffer**: `ArrayBuffer` methods like `byteLength` and `slice` explicitly check if the buffer is a `SharedArrayBuffer` and throw a `TypeError` if it is [29](#ref-q1-29)  [30](#ref-q1-30) . The `SharedArrayBuffer` is a distinct type [31](#ref-q1-31) .
*   **Resizable/Growable Buffers**: While `ArrayBuffer` supports `max_byte_len` for resizable buffers [32](#ref-q1-32) , the `resize` method explicitly checks if the buffer is fixed-length before allowing resizing [33](#ref-q1-33) .
*   **Transfer Semantics**: Transfer methods (`transfer`, `transferToFixedLength`) are guarded by an experimental feature flag [34](#ref-q1-34) , indicating they are not part of the ordinary `ArrayBuffer` semantics by default.
*   **TypedArray Transform Methods**: The prompt does not mention `TypedArray` transform methods, and the provided context for `ArrayBuffer` and `DataView` does not show any direct integration or scope creep into these methods. `TypedArray` objects are views over `ArrayBuffer`s, but their specific transform methods are handled within the `TypedArray` implementation [35](#ref-q1-35) .

## Implementation-Relevant Details and Tests

The `ArrayBuffer` and `DataView` implementations are built upon the `JsData` trait [36](#ref-q1-36)  [37](#ref-q1-37) , and their methods are registered using `BuiltInBuilder` [38](#ref-q1-38)  [39](#ref-q1-39) .

Testing for `ArrayBuffer` includes checks for `detach` functionality and conversion to `Vec<u8>` [40](#ref-q1-40) . The `js262` test harness also provides a `$262.detachArrayBuffer()` function to test the detachment behavior [41](#ref-q1-41) .

## Notes

The `AlignedVec` type is used for the underlying data storage of `ArrayBuffer` [42](#ref-q1-42) , which ensures proper memory alignment for efficient access. The `DataView` implementation includes methods for getting and setting various numeric types (`getBigInt64`, `setFloat32`, etc.) [43](#ref-q1-43) , which involve endianness handling [44](#ref-q1-44)  [45](#ref-q1-45) .

Wiki pages you might want to explore:
- [Standard Builtins (boa-dev/boa)](/wiki/boa-dev/boa#3.1)
- [Glossary (boa-dev/boa)](/wiki/boa-dev/boa#7)

View this search on DeepWiki: https://deepwiki.com/search/for-implementing-ecmascript-or_a7d60b37-9322-46df-a259-cc0dbd9f8cd4

## References

<a id="ref-q1-1"></a>
### [1] `core/engine/src/builtins/array_buffer/mod.rs:200-202`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L200-L202)

```rust
/// The internal representation of an `ArrayBuffer` object.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct ArrayBuffer {
```

<a id="ref-q1-2"></a>
### [2] `core/engine/src/builtins/array_buffer/mod.rs:429-451`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L429-L451)

```rust
    fn constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. If NewTarget is undefined, throw a TypeError exception.
        if new_target.is_undefined() {
            return Err(JsNativeError::typ()
                .with_message("ArrayBuffer.constructor called with undefined new target")
                .into());
        }

        // 2. Let byteLength be ? ToIndex(length).
        let byte_len = args.get_or_undefined(0).to_index(context)?;

        // 3. Let requestedMaxByteLength be ? GetArrayBufferMaxByteLengthOption(options).
        let max_byte_len = get_max_byte_len(args.get_or_undefined(1), context)?;

        // 4. Return ? AllocateArrayBuffer(NewTarget, byteLength, requestedMaxByteLength).
        Ok(Self::allocate(new_target, byte_len, max_byte_len, context)?
            .upcast()
            .into())
    }
```

<a id="ref-q1-3"></a>
### [3] `core/engine/src/builtins/array_buffer/mod.rs:442-445`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L442-L445)

```rust
        let byte_len = args.get_or_undefined(0).to_index(context)?;

        // 3. Let requestedMaxByteLength be ? GetArrayBufferMaxByteLengthOption(options).
        let max_byte_len = get_max_byte_len(args.get_or_undefined(1), context)?;
```

<a id="ref-q1-4"></a>
### [4] `core/engine/src/builtins/array_buffer/mod.rs:868-917`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L868-L917)

```rust
    pub(crate) fn allocate(
        constructor: &JsValue,
        byte_len: u64,
        max_byte_len: Option<u64>,
        context: &mut Context,
    ) -> JsResult<JsObject<ArrayBuffer>> {
        // 1. Let slots be « [[ArrayBufferData]], [[ArrayBufferByteLength]], [[ArrayBufferDetachKey]] ».
        // 2. If maxByteLength is present and maxByteLength is not empty, let allocatingResizableBuffer be true; otherwise let allocatingResizableBuffer be false.
        // 3. If allocatingResizableBuffer is true, then
        //     a. If byteLength > maxByteLength, throw a RangeError exception.
        //     b. Append [[ArrayBufferMaxByteLength]] to slots.
        if let Some(max_byte_len) = max_byte_len
            && byte_len > max_byte_len
        {
            return Err(JsNativeError::range()
                .with_message("`length` cannot be bigger than `maxByteLength`")
                .into());
        }

        // 4. Let obj be ? OrdinaryCreateFromConstructor(constructor, "%ArrayBuffer.prototype%", slots).
        let prototype = get_prototype_from_constructor(
            constructor,
            StandardConstructors::array_buffer,
            context,
        )?;

        // 5. Let block be ? CreateByteDataBlock(byteLength).
        // Preemptively allocate for `max_byte_len` if possible.
        //     a. If it is not possible to create a Data Block block consisting of maxByteLength bytes, throw a RangeError exception.
        //     b. NOTE: Resizable ArrayBuffers are designed to be implementable with in-place growth. Implementations may
        //        throw if, for example, virtual memory cannot be reserved up front.
        let block = create_byte_data_block(byte_len, max_byte_len, context)?;

        let obj = JsObject::new(
            context.root_shape(),
            prototype,
            Self {
                // 6. Set obj.[[ArrayBufferData]] to block.
                // 7. Set obj.[[ArrayBufferByteLength]] to byteLength.
                data: Some(block),
                // 8. If allocatingResizableBuffer is true, then
                //    c. Set obj.[[ArrayBufferMaxByteLength]] to maxByteLength.
                max_byte_len,
                detach_key: JsValue::undefined(),
            },
        );

        // 9. Return obj.
        Ok(obj)
    }
```

<a id="ref-q1-5"></a>
### [5] `core/engine/src/builtins/array_buffer/mod.rs:491-512`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L491-L512)

```rust
    pub(crate) fn get_byte_length(
        this: &JsValue,
        _args: &[JsValue],
        _: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Let O be the this value.
        // 2. Perform ? RequireInternalSlot(O, [[ArrayBufferData]]).
        // 3. If IsSharedArrayBuffer(O) is true, throw a TypeError exception.
        let object = this.as_object();
        let buf = object
            .as_ref()
            .and_then(JsObject::downcast_ref::<Self>)
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("get ArrayBuffer.prototype.byteLength called with invalid `this`")
            })?;

        // 4. If IsDetachedBuffer(O) is true, return +0𝔽.
        // 5. Let length be O.[[ArrayBufferByteLength]].
        // 6. Return 𝔽(length).
        Ok(buf.len().into())
    }
```

<a id="ref-q1-6"></a>
### [6] `core/engine/src/builtins/array_buffer/mod.rs:548-570`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L548-L570)

```rust
    /// [`get ArrayBuffer.prototype.resizable`][spec].
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-get-arraybuffer.prototype.resizable
    pub(crate) fn get_resizable(
        this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Let O be the this value.
        // 2. Perform ? RequireInternalSlot(O, [[ArrayBufferData]]).
        // 3. If IsSharedArrayBuffer(O) is true, throw a TypeError exception.
        let object = this.as_object();
        let buf = object
            .as_ref()
            .and_then(JsObject::downcast_ref::<Self>)
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("get ArrayBuffer.prototype.resizable called with invalid `this`")
            })?;

        // 4. If IsFixedLengthArrayBuffer(O) is false, return true; otherwise return false.
        Ok(JsValue::from(!buf.is_fixed_len()))
    }
```

<a id="ref-q1-7"></a>
### [7] `core/engine/src/builtins/array_buffer/mod.rs:517-545`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L517-L545)

```rust
    pub(crate) fn get_max_byte_length(
        this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Let O be the this value.
        // 2. Perform ? RequireInternalSlot(O, [[ArrayBufferData]]).
        // 3. If IsSharedArrayBuffer(O) is true, throw a TypeError exception.
        let object = this.as_object();
        let buf = object
            .as_ref()
            .and_then(JsObject::downcast_ref::<Self>)
            .ok_or_else(|| {
                JsNativeError::typ().with_message(
                    "get ArrayBuffer.prototype.maxByteLength called with invalid `this`",
                )
            })?;

        // 4. If IsDetachedBuffer(O) is true, return +0𝔽.
        let Some(data) = buf.bytes() else {
            return Ok(JsValue::from(0));
        };

        // 5. If IsFixedLengthArrayBuffer(O) is true, then
        //     a. Let length be O.[[ArrayBufferByteLength]].
        // 6. Else,
        //     a. Let length be O.[[ArrayBufferMaxByteLength]].
        // 7. Return 𝔽(length).
        Ok(buf.max_byte_len.unwrap_or(data.len() as u64).into())
```

<a id="ref-q1-8"></a>
### [8] `core/engine/src/builtins/array_buffer/mod.rs:648-746`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L648-L746)

```rust
    fn slice(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        // 1. Let O be the this value.
        // 2. Perform ? RequireInternalSlot(O, [[ArrayBufferData]]).
        // 3. If IsSharedArrayBuffer(O) is true, throw a TypeError exception.
        let buf = this
            .as_object()
            .and_then(|o| o.clone().downcast::<Self>().ok())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("ArrayBuffer.slice called with invalid `this` value")
            })?;

        let len = {
            let buf = buf.borrow();
            // 4. If IsDetachedBuffer(O) is true, throw a TypeError exception.
            if buf.data().is_detached() {
                return Err(JsNativeError::typ()
                    .with_message("ArrayBuffer.slice called with detached buffer")
                    .into());
            }
            // 5. Let len be O.[[ArrayBufferByteLength]].
            buf.data().len() as u64
        };

        // 6. Let relativeStart be ? ToIntegerOrInfinity(start).
        // 7. If relativeStart = -∞, let first be 0.
        // 8. Else if relativeStart < 0, let first be max(len + relativeStart, 0).
        // 9. Else, let first be min(relativeStart, len).
        let first = Array::get_relative_start(context, args.get_or_undefined(0), len)?;

        // 10. If end is undefined, let relativeEnd be len; else let relativeEnd be ? ToIntegerOrInfinity(end).
        // 11. If relativeEnd = -∞, let final be 0.
        // 12. Else if relativeEnd < 0, let final be max(len + relativeEnd, 0).
        // 13. Else, let final be min(relativeEnd, len).
        let final_ = Array::get_relative_end(context, args.get_or_undefined(1), len)?;

        // 14. Let newLen be max(final - first, 0).
        let new_len = final_.saturating_sub(first);

        // 15. Let ctor be ? SpeciesConstructor(O, %ArrayBuffer%).
        let ctor = buf
            .clone()
            .upcast()
            .species_constructor(StandardConstructors::array_buffer, context)?;

        // 16. Let new be ? Construct(ctor, « 𝔽(newLen) »).
        let new = ctor.construct(&[new_len.into()], Some(&ctor), context)?;

        // 17. Perform ? RequireInternalSlot(new, [[ArrayBufferData]]).
        // 18. If IsSharedArrayBuffer(new) is true, throw a TypeError exception.
        let Ok(new) = new.downcast::<Self>() else {
            return Err(JsNativeError::typ()
                .with_message("ArrayBuffer constructor returned invalid object")
                .into());
        };

        // 20. If SameValue(new, O) is true, throw a TypeError exception.
        if JsObject::equals(&buf, &new) {
            return Err(JsNativeError::typ()
                .with_message("new ArrayBuffer is the same as this ArrayBuffer")
                .into());
        }

        {
            // 19. If IsDetachedBuffer(new) is true, throw a TypeError exception.
            // 25. Let toBuf be new.[[ArrayBufferData]].
            let mut new = new.borrow_mut();
            let Some(to_buf) = new.data_mut().bytes_mut() else {
                return Err(JsNativeError::typ()
                    .with_message("ArrayBuffer constructor returned detached ArrayBuffer")
                    .into());
            };

            // 21. If new.[[ArrayBufferByteLength]] < newLen, throw a TypeError exception.
            if (to_buf.len() as u64) < new_len {
                return Err(JsNativeError::typ()
                    .with_message("new ArrayBuffer length too small")
                    .into());
            }

            // 22. NOTE: Side-effects of the above steps may have detached O.
            // 23. If IsDetachedBuffer(O) is true, throw a TypeError exception.
            // 24. Let fromBuf be O.[[ArrayBufferData]].
            let buf = buf.borrow();
            let Some(from_buf) = buf.data().bytes() else {
                return Err(JsNativeError::typ()
                    .with_message("ArrayBuffer detached while ArrayBuffer.slice was running")
                    .into());
            };

            // 26. Perform CopyDataBlockBytes(toBuf, 0, fromBuf, first, newLen).
            let first = first as usize;
            let new_len = new_len as usize;
            to_buf[..new_len].copy_from_slice(&from_buf[first..first + new_len]);
        }

        // 27. Return new.
        Ok(new.upcast().into())
    }
```

<a id="ref-q1-9"></a>
### [9] `core/engine/src/builtins/array_buffer/mod.rs:479-483`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L479-L483)

```rust
    #[allow(clippy::unnecessary_wraps)]
    fn get_species(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
        // 1. Return the this value.
        Ok(this.clone())
    }
```

<a id="ref-q1-10"></a>
### [10] `core/engine/src/builtins/array_buffer/mod.rs:455-471`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L455-L471)

```rust
    /// `ArrayBuffer.isView ( arg )`
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-arraybuffer.isview
    #[allow(clippy::unnecessary_wraps)]
    fn is_view(_: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
        // 1. If Type(arg) is not Object, return false.
        // 2. If arg has a [[ViewedArrayBuffer]] internal slot, return true.
        // 3. Return false.
        Ok(args
            .get_or_undefined(0)
            .as_object()
            .is_some_and(|obj| obj.is::<TypedArray>() || obj.is::<DataView>())
            .into())
    }
```

<a id="ref-q1-11"></a>
### [11] `core/engine/src/builtins/array_buffer/mod.rs:463-470`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L463-L470)

```rust
        // 1. If Type(arg) is not Object, return false.
        // 2. If arg has a [[ViewedArrayBuffer]] internal slot, return true.
        // 3. Return false.
        Ok(args
            .get_or_undefined(0)
            .as_object()
            .is_some_and(|obj| obj.is::<TypedArray>() || obj.is::<DataView>())
            .into())
```

<a id="ref-q1-12"></a>
### [12] `core/engine/src/builtins/dataview/mod.rs:37-39`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L37-L39)

```rust
/// The internal representation of a `DataView` object.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct DataView {
```

<a id="ref-q1-13"></a>
### [13] `core/engine/src/builtins/dataview/mod.rs:40-42`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L40-L42)

```rust
    pub(crate) viewed_array_buffer: BufferObject,
    pub(crate) byte_length: Option<u64>,
    pub(crate) byte_offset: u64,
```

<a id="ref-q1-14"></a>
### [14] `core/engine/src/builtins/dataview/mod.rs:40`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L40)

```rust
    pub(crate) viewed_array_buffer: BufferObject,
```

<a id="ref-q1-15"></a>
### [15] `core/engine/src/builtins/dataview/mod.rs:41`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L41)

```rust
    pub(crate) byte_length: Option<u64>,
```

<a id="ref-q1-16"></a>
### [16] `core/engine/src/builtins/dataview/mod.rs:42`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L42)

```rust
    pub(crate) byte_offset: u64,
```

<a id="ref-q1-17"></a>
### [17] `core/engine/src/builtins/dataview/mod.rs:193-312`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L193-L312)

```rust
    fn constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. If NewTarget is undefined, throw a TypeError exception.
        if new_target.is_undefined() {
            return Err(JsNativeError::typ()
                .with_message("cannot call `DataView` constructor without `new`")
                .into());
        }
        let byte_len = args.get_or_undefined(2);

        // 2. Perform ? RequireInternalSlot(buffer, [[ArrayBufferData]]).
        let buffer = args
            .get_or_undefined(0)
            .as_object()
            .and_then(|o| o.clone().into_buffer_object().ok())
            .ok_or_else(|| JsNativeError::typ().with_message("buffer must be an ArrayBuffer"))?;

        // 3. Let offset be ? ToIndex(byteOffset).
        let offset = args.get_or_undefined(1).to_index(context)?;

        let (buf_byte_len, is_fixed_len) = {
            let buffer = buffer.as_buffer();

            // 4. If IsDetachedBuffer(buffer) is true, throw a TypeError exception.
            let Some(slice) = buffer.bytes(Ordering::SeqCst) else {
                return Err(JsNativeError::typ()
                    .with_message("ArrayBuffer is detached")
                    .into());
            };

            // 5. Let bufferByteLength be ArrayBufferByteLength(buffer, seq-cst).
            let buf_len = slice.len() as u64;

            // 6. If offset > bufferByteLength, throw a RangeError exception.
            if offset > buf_len {
                return Err(JsNativeError::range()
                    .with_message("Start offset is outside the bounds of the buffer")
                    .into());
            }

            // 7. Let bufferIsFixedLength be IsFixedLengthArrayBuffer(buffer).

            (buf_len, buffer.is_fixed_len())
        };

        // 8. If byteLength is undefined, then
        let view_byte_len = if byte_len.is_undefined() {
            // a. If bufferIsFixedLength is true, then
            //     i. Let viewByteLength be bufferByteLength - offset.
            // b. Else,
            //     i. Let viewByteLength be auto.
            is_fixed_len.then_some(buf_byte_len - offset)
        } else {
            // 9. Else,
            //     a. Let viewByteLength be ? ToIndex(byteLength).
            let byte_len = byte_len.to_index(context)?;

            //     b. If offset + viewByteLength > bufferByteLength, throw a RangeError exception.
            if offset + byte_len > buf_byte_len {
                return Err(JsNativeError::range()
                    .with_message("Invalid data view length")
                    .into());
            }
            Some(byte_len)
        };

        // 10. Let O be ? OrdinaryCreateFromConstructor(NewTarget, "%DataView.prototype%",
        //     « [[DataView]], [[ViewedArrayBuffer]], [[ByteLength]], [[ByteOffset]] »).
        let prototype =
            get_prototype_from_constructor(new_target, StandardConstructors::data_view, context)?;

        // 11. If IsDetachedBuffer(buffer) is true, throw a TypeError exception.
        // 12. Set bufferByteLength to ArrayBufferByteLength(buffer, seq-cst).
        let Some(buf_byte_len) = buffer
            .as_buffer()
            .bytes(Ordering::SeqCst)
            .map(|s| s.len() as u64)
        else {
            return Err(JsNativeError::typ()
                .with_message("ArrayBuffer can't be detached")
                .into());
        };

        // 13. If offset > bufferByteLength, throw a RangeError exception.
        if offset > buf_byte_len {
            return Err(JsNativeError::range()
                .with_message("DataView offset outside of buffer array bounds")
                .into());
        }

        // 14. If byteLength is not undefined, then
        //     a. If offset + viewByteLength > bufferByteLength, throw a RangeError exception.
        if !byte_len.is_undefined()
            && let Some(view_byte_len) = view_byte_len
            && offset + view_byte_len > buf_byte_len
        {
            return Err(JsNativeError::range()
                .with_message("DataView offset outside of buffer array bounds")
                .into());
        }

        let obj = JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            Self {
                // 15. Set O.[[ViewedArrayBuffer]] to buffer.
                viewed_array_buffer: buffer,
                // 16. Set O.[[ByteLength]] to viewByteLength.
                byte_length: view_byte_len,
                // 17. Set O.[[ByteOffset]] to offset.
                byte_offset: offset,
            },
        );

        // 18. Return O.
        Ok(obj.into())
    }
```

<a id="ref-q1-18"></a>
### [18] `core/engine/src/builtins/dataview/mod.rs:262-266`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L262-L266)

```rust
        // 10. Let O be ? OrdinaryCreateFromConstructor(NewTarget, "%DataView.prototype%",
        //     « [[DataView]], [[ViewedArrayBuffer]], [[ByteLength]], [[ByteOffset]] »).
        let prototype =
            get_prototype_from_constructor(new_target, StandardConstructors::data_view, context)?;
```

<a id="ref-q1-19"></a>
### [19] `core/engine/src/builtins/array_buffer/mod.rs:499-506`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L499-L506)

```rust
        let object = this.as_object();
        let buf = object
            .as_ref()
            .and_then(JsObject::downcast_ref::<Self>)
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("get ArrayBuffer.prototype.byteLength called with invalid `this`")
            })?;
```

<a id="ref-q1-20"></a>
### [20] `core/engine/src/builtins/array_buffer/mod.rs:508`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L508)

```rust
        // 4. If IsDetachedBuffer(O) is true, return +0𝔽.
```

<a id="ref-q1-21"></a>
### [21] `core/engine/src/builtins/array_buffer/mod.rs:363-367`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L363-L367)

```rust
                js_string!("byteLength"),
                Some(get_byte_length),
                None,
                flag_attributes,
            )
```

<a id="ref-q1-22"></a>
### [22] `core/engine/src/builtins/array_buffer/mod.rs:642-647`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L642-L647)

```rust
    /// `ArrayBuffer.prototype.slice ( start, end )`
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-arraybuffer.prototype.slice
```

<a id="ref-q1-23"></a>
### [23] `core/engine/src/builtins/array_buffer/mod.rs:652-658`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L652-L658)

```rust
        let buf = this
            .as_object()
            .and_then(|o| o.clone().downcast::<Self>().ok())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("ArrayBuffer.slice called with invalid `this` value")
            })?;
```

<a id="ref-q1-24"></a>
### [24] `core/engine/src/builtins/array_buffer/mod.rs:663-666`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L663-L666)

```rust
            if buf.data().is_detached() {
                return Err(JsNativeError::typ()
                    .with_message("ArrayBuffer.slice called with detached buffer")
                    .into());
```

<a id="ref-q1-25"></a>
### [25] `core/engine/src/builtins/array_buffer/mod.rs:672-682`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L672-L682)

```rust
        // 6. Let relativeStart be ? ToIntegerOrInfinity(start).
        // 7. If relativeStart = -∞, let first be 0.
        // 8. Else if relativeStart < 0, let first be max(len + relativeStart, 0).
        // 9. Else, let first be min(relativeStart, len).
        let first = Array::get_relative_start(context, args.get_or_undefined(0), len)?;

        // 10. If end is undefined, let relativeEnd be len; else let relativeEnd be ? ToIntegerOrInfinity(end).
        // 11. If relativeEnd = -∞, let final be 0.
        // 12. Else if relativeEnd < 0, let final be max(len + relativeEnd, 0).
        // 13. Else, let final be min(relativeEnd, len).
        let final_ = Array::get_relative_end(context, args.get_or_undefined(1), len)?;
```

<a id="ref-q1-26"></a>
### [26] `core/engine/src/builtins/array_buffer/mod.rs:678`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L678)

```rust
        // 10. If end is undefined, let relativeEnd be len; else let relativeEnd be ? ToIntegerOrInfinity(end).
```

<a id="ref-q1-27"></a>
### [27] `core/engine/src/builtins/array_buffer/mod.rs:687-691`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L687-L691)

```rust
        // 15. Let ctor be ? SpeciesConstructor(O, %ArrayBuffer%).
        let ctor = buf
            .clone()
            .upcast()
            .species_constructor(StandardConstructors::array_buffer, context)?;
```

<a id="ref-q1-28"></a>
### [28] `core/engine/src/builtins/array_buffer/mod.rs:738-741`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L738-L741)

```rust
            // 26. Perform CopyDataBlockBytes(toBuf, 0, fromBuf, first, newLen).
            let first = first as usize;
            let new_len = new_len as usize;
            to_buf[..new_len].copy_from_slice(&from_buf[first..first + new_len]);
```

<a id="ref-q1-29"></a>
### [29] `core/engine/src/builtins/array_buffer/mod.rs:498`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L498)

```rust
        // 3. If IsSharedArrayBuffer(O) is true, throw a TypeError exception.
```

<a id="ref-q1-30"></a>
### [30] `core/engine/src/builtins/array_buffer/mod.rs:651`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L651)

```rust
        // 3. If IsSharedArrayBuffer(O) is true, throw a TypeError exception.
```

<a id="ref-q1-31"></a>
### [31] `core/engine/src/builtins/array_buffer/mod.rs:22`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L22)

```rust
pub use shared::SharedArrayBuffer;
```

<a id="ref-q1-32"></a>
### [32] `core/engine/src/builtins/array_buffer/mod.rs:207`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L207)

```rust
    /// The `[[ArrayBufferMaxByteLength]]` internal slot.
```

<a id="ref-q1-33"></a>
### [33] `core/engine/src/builtins/array_buffer/mod.rs:271-275`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L271-L275)

```rust
    pub fn resize(&mut self, new_byte_length: u64) -> JsResult<()> {
        let Some(max_byte_len) = self.max_byte_len else {
            return Err(JsNativeError::typ()
                .with_message("ArrayBuffer.resize: cannot resize a fixed-length buffer")
                .into());
```

<a id="ref-q1-34"></a>
### [34] `core/engine/src/builtins/array_buffer/mod.rs:396-401`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L396-L401)

```rust
            .method(Self::transfer::<false>, js_string!("transfer"), 0)
            .method(
                Self::transfer::<true>,
                js_string!("transferToFixedLength"),
                0,
            );
```

<a id="ref-q1-35"></a>
### [35] `core/engine/src/builtins/typed_array/object.rs:31-37`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/typed_array/object.rs#L31-L37)

<a id="ref-q1-36"></a>
### [36] `core/engine/src/builtins/array_buffer/mod.rs:201`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L201)

```rust
#[derive(Debug, Clone, Trace, Finalize, JsData)]
```

<a id="ref-q1-37"></a>
### [37] `core/engine/src/builtins/dataview/mod.rs:38`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L38)

```rust
#[derive(Debug, Clone, Trace, Finalize, JsData)]
```

<a id="ref-q1-38"></a>
### [38] `core/engine/src/builtins/array_buffer/mod.rs:354-403`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L354-L403)

```rust
        let builder = BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .static_accessor(
                JsSymbol::species(),
                Some(get_species),
                None,
                Attribute::CONFIGURABLE,
            )
            .static_method(Self::is_view, js_string!("isView"), 1)
            .accessor(
                js_string!("byteLength"),
                Some(get_byte_length),
                None,
                flag_attributes,
            )
            .accessor(
                js_string!("resizable"),
                Some(get_resizable),
                None,
                flag_attributes,
            )
            .accessor(
                js_string!("maxByteLength"),
                Some(get_max_byte_length),
                None,
                flag_attributes,
            )
            .method(Self::js_resize, js_string!("resize"), 1)
            .method(Self::slice, js_string!("slice"), 2)
            .property(
                JsSymbol::to_string_tag(),
                Self::NAME,
                Attribute::READONLY | Attribute::NON_ENUMERABLE | Attribute::CONFIGURABLE,
            );

        #[cfg(feature = "experimental")]
        let builder = builder
            .accessor(
                js_string!("detached"),
                Some(get_detached),
                None,
                flag_attributes,
            )
            .method(Self::transfer::<false>, js_string!("transfer"), 0)
            .method(
                Self::transfer::<true>,
                js_string!("transferToFixedLength"),
                0,
            );

        builder.build();
```

<a id="ref-q1-39"></a>
### [39] `core/engine/src/builtins/dataview/mod.rs:112-162`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L112-L162)

```rust
        let builder = BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .accessor(
                js_string!("buffer"),
                Some(get_buffer),
                None,
                flag_attributes,
            )
            .accessor(
                js_string!("byteLength"),
                Some(get_byte_length),
                None,
                flag_attributes,
            )
            .accessor(
                js_string!("byteOffset"),
                Some(get_byte_offset),
                None,
                flag_attributes,
            )
            .method(Self::get_big_int64, js_string!("getBigInt64"), 1)
            .method(Self::get_big_uint64, js_string!("getBigUint64"), 1)
            .method(Self::get_float32, js_string!("getFloat32"), 1)
            .method(Self::get_float64, js_string!("getFloat64"), 1)
            .method(Self::get_int8, js_string!("getInt8"), 1)
            .method(Self::get_int16, js_string!("getInt16"), 1)
            .method(Self::get_int32, js_string!("getInt32"), 1)
            .method(Self::get_uint8, js_string!("getUint8"), 1)
            .method(Self::get_uint16, js_string!("getUint16"), 1)
            .method(Self::get_uint32, js_string!("getUint32"), 1)
            .method(Self::set_big_int64, js_string!("setBigInt64"), 2)
            .method(Self::set_big_uint64, js_string!("setBigUint64"), 2)
            .method(Self::set_float32, js_string!("setFloat32"), 2)
            .method(Self::set_float64, js_string!("setFloat64"), 2)
            .method(Self::set_int8, js_string!("setInt8"), 2)
            .method(Self::set_int16, js_string!("setInt16"), 2)
            .method(Self::set_int32, js_string!("setInt32"), 2)
            .method(Self::set_uint8, js_string!("setUint8"), 2)
            .method(Self::set_uint16, js_string!("setUint16"), 2)
            .method(Self::set_uint32, js_string!("setUint32"), 2)
            .property(
                JsSymbol::to_string_tag(),
                Self::NAME,
                Attribute::READONLY | Attribute::NON_ENUMERABLE | Attribute::CONFIGURABLE,
            );

        #[cfg(feature = "float16")]
        let builder = builder
            .method(Self::get_float16, js_string!("getFloat16"), 1)
            .method(Self::set_float16, js_string!("setFloat16"), 2);

        builder.build();
```

<a id="ref-q1-40"></a>
### [40] `core/engine/src/object/builtins/jsarraybuffer.rs:365-374`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/builtins/jsarraybuffer.rs#L365-L374)

```rust
        let context = &mut Context::default();

        let data = AlignedVec::from_iter(0, [1u8, 2, 3, 4, 5]);
        let buffer = JsArrayBuffer::from_byte_block(data, context).unwrap();

        assert_eq!(buffer.to_vec(), Some(vec![1u8, 2, 3, 4, 5]));

        buffer.detach(&JsValue::undefined()).unwrap();
        assert_eq!(buffer.to_vec(), None);
    }
```

<a id="ref-q1-41"></a>
### [41] `tests/tester/src/exec/js262.rs:147-172`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/tests/tester/src/exec/js262.rs#L147-L172)

```rust
/// The `$262.detachArrayBuffer()` function.
///
/// Implements the `DetachArrayBuffer` abstract operation.
fn detach_array_buffer(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    fn type_err() -> JsNativeError {
        JsNativeError::typ().with_message("The provided object was not an ArrayBuffer")
    }

    // 1. Assert: IsSharedArrayBuffer(arrayBuffer) is false.
    let object = args.first().and_then(JsValue::as_object);
    let mut array_buffer = object
        .as_ref()
        .and_then(|o| o.downcast_mut::<ArrayBuffer>())
        .ok_or_else(type_err)?;

    // 2. If key is not present, set key to undefined.
    let key = args.get_or_undefined(1);

    // 3. If SameValue(arrayBuffer.[[ArrayBufferDetachKey]], key) is false, throw a TypeError exception.
    // 4. Set arrayBuffer.[[ArrayBufferData]] to null.
    // 5. Set arrayBuffer.[[ArrayBufferByteLength]] to 0.
    array_buffer.detach(key)?;

    // 6. Return NormalCompletion(null).
    Ok(JsValue::null())
}
```

<a id="ref-q1-42"></a>
### [42] `core/engine/src/builtins/array_buffer/mod.rs:205`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array_buffer/mod.rs#L205)

```rust
    data: Option<AlignedVec<u8>>,
```

<a id="ref-q1-43"></a>
### [43] `core/engine/src/builtins/dataview/mod.rs:131-150`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L131-L150)

```rust
            .method(Self::get_big_int64, js_string!("getBigInt64"), 1)
            .method(Self::get_big_uint64, js_string!("getBigUint64"), 1)
            .method(Self::get_float32, js_string!("getFloat32"), 1)
            .method(Self::get_float64, js_string!("getFloat64"), 1)
            .method(Self::get_int8, js_string!("getInt8"), 1)
            .method(Self::get_int16, js_string!("getInt16"), 1)
            .method(Self::get_int32, js_string!("getInt32"), 1)
            .method(Self::get_uint8, js_string!("getUint8"), 1)
            .method(Self::get_uint16, js_string!("getUint16"), 1)
            .method(Self::get_uint32, js_string!("getUint32"), 1)
            .method(Self::set_big_int64, js_string!("setBigInt64"), 2)
            .method(Self::set_big_uint64, js_string!("setBigUint64"), 2)
            .method(Self::set_float32, js_string!("setFloat32"), 2)
            .method(Self::set_float64, js_string!("setFloat64"), 2)
            .method(Self::set_int8, js_string!("setInt8"), 2)
            .method(Self::set_int16, js_string!("setInt16"), 2)
            .method(Self::set_int32, js_string!("setInt32"), 2)
            .method(Self::set_uint8, js_string!("setUint8"), 2)
            .method(Self::set_uint16, js_string!("setUint16"), 2)
            .method(Self::set_uint32, js_string!("setUint32"), 2)
```

<a id="ref-q1-44"></a>
### [44] `core/engine/src/builtins/dataview/mod.rs:509-512`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L509-L512)

```rust
            if is_little_endian {
                value.to_little_endian()
            } else {
                value.to_big_endian()
```

<a id="ref-q1-45"></a>
### [45] `core/engine/src/builtins/dataview/mod.rs:849-852`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/dataview/mod.rs#L849-L852)

```rust
            let value = if is_little_endian {
                value.to_little_endian()
            } else {
                value.to_big_endian()
```
