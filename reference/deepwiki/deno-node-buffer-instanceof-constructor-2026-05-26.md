# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility, how should Buffer instances created by Buffer.from or Buffer.prototype.slice behave for instanceof Buffer and the constructor.name property? Does subclassing Buffer make MyBuffer.from return subclass instances or plain Buffer instances?
Search: https://deepwiki.com/search/for-node-compatibility-how-sho_ba39f09a-383c-400b-9f61-a31c213cdef0

**Question**: For Node compatibility, how should Buffer instances created by Buffer.from or Buffer.prototype.slice behave for instanceof Buffer and the constructor.name property? Does subclassing Buffer make MyBuffer.from return subclass instances or plain Buffer instances?

For Node compatibility, `Buffer` instances created by `Buffer.from()` or `Buffer.prototype.slice()` will behave as `Buffer` instances for `instanceof Buffer` checks and will have a `constructor.name` of "Buffer" [1](#ref-q1-1) . Subclassing `Buffer` will not make `MyBuffer.from()` return subclass instances; instead, it will return plain `Buffer` instances [2](#ref-q1-2) .

## `instanceof Buffer` and `constructor.name` Behavior

The `Buffer` class in Deno's Node compatibility layer is implemented as a constructor function that delegates to `FastBuffer`, which extends `Uint8Array` [3](#ref-q1-3) . To ensure `instanceof Buffer` checks work correctly, `Buffer.prototype` is set to `FastBuffer.prototype` [4](#ref-q1-4) . The `Buffer.isBuffer()` static method also checks if an object's prototype is `Buffer.prototype` [5](#ref-q1-5) .

For the `constructor.name` property, the `inspect` method on `Buffer.prototype` explicitly attempts to retrieve the `constructor.name` of the instance [1](#ref-q1-1) . If successful, it uses that name; otherwise, it defaults to "Buffer" [6](#ref-q1-6) .

## `Buffer.from()` and Subclassing

The `Buffer.from()` method is defined as `BufferFrom` and internally calls the `_from` function [2](#ref-q1-2) . The `_from` function, when creating a new buffer from various inputs (string, ArrayBuffer, etc.), ultimately calls `createBuffer()` or directly instantiates `FastBuffer` [7](#ref-q1-7) . For example, `createBuffer()` returns `new FastBuffer(length)` [8](#ref-q1-8) .

Similarly, `Buffer.prototype.slice()` is implemented by calling `this.subarray()` [9](#ref-q1-9) . The `subarray()` method then creates a new `FastBuffer` instance using the underlying `ArrayBuffer` of the original buffer [10](#ref-q1-10) .

Because `Buffer.from()` and `Buffer.prototype.slice()` explicitly create instances of `FastBuffer` (which is the internal representation of `Buffer`), subclassing `Buffer` will not result in instances of the subclass being returned by these methods. They will always return plain `Buffer` instances (i.e., `FastBuffer` instances).

## Notes

The `Buffer` implementation in Deno's Node compatibility layer uses a `FastBuffer` class that extends `Uint8Array` for performance and to align with Web standards [3](#ref-q1-3) . The `Buffer` constructor itself is a wrapper that delegates to `FastBuffer` and emits a deprecation warning [11](#ref-q1-11) . This architecture ensures Node.js compatibility while leveraging `Uint8Array`'s native performance.

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)
- [Buffer and Core APIs (denoland/deno)](/wiki/denoland/deno#7.2)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-how-sho_ba39f09a-383c-400b-9f61-a31c213cdef0

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/internal/buffer.mjs:851-858`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L851-L858)

```
      let constructorName = "Buffer";
      try {
        const { constructor } = this;
        if (
          typeof constructor === "function" &&
          ObjectPrototypeHasOwnProperty(constructor, "name")
        ) {
          constructorName = constructor.name;
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/internal/buffer.mjs:334-340`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L334-L340)

```
const BufferFrom = Buffer.from = function from(
  value,
  encodingOrOffset,
  length,
) {
  return _from(value, encodingOrOffset, length);
};
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/internal/buffer.mjs:197-201`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L197-L201)

```
class FastBuffer extends Uint8Array {
  constructor(bufferOrLength, byteOffset, length) {
    super(bufferOrLength, byteOffset, length);
  }
}
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/internal/buffer.mjs:204`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L204)

```
Buffer.prototype = FastBuffer.prototype;
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/internal/buffer.mjs:547-549`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L547-L549)

```
const BufferIsBuffer = Buffer.isBuffer = function isBuffer(b) {
  return ObjectPrototypeIsPrototypeOf(Buffer.prototype, b);
};
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/internal/buffer.mjs:860`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L860)

```
      } catch {
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/internal/buffer.mjs:294-331`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L294-L331)

```
function _from(value, encodingOrOffset, length) {
  if (typeof value === "string") {
    return fromString(value, encodingOrOffset);
  }

  if (typeof value === "object" && value !== null) {
    if (isAnyArrayBuffer(value)) {
      return fromArrayBuffer(value, encodingOrOffset, length);
    }

    // deno-lint-ignore prefer-primordials
    const valueOf = value.valueOf && value.valueOf();
    if (
      valueOf != null &&
      valueOf !== value &&
      (typeof valueOf === "string" || typeof valueOf === "object")
    ) {
      return _from(valueOf, encodingOrOffset, length);
    }

    const b = fromObject(value);
    if (b) {
      return b;
    }

    if (typeof value[SymbolToPrimitive] === "function") {
      const primitive = value[SymbolToPrimitive]("string");
      if (typeof primitive === "string") {
        return fromString(primitive, encodingOrOffset);
      }
    }
  }

  throw new codes.ERR_INVALID_ARG_TYPE(
    "first argument",
    ["string", "Buffer", "ArrayBuffer", "Array", "Array-like Object"],
    value,
  );
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/internal/buffer.mjs:237-244`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L237-L244)

```
function createBuffer(length) {
  if (length > kMaxLength) {
    throw new RangeError(
      'The value "' + length + '" is invalid for option "size"',
    );
  }

  return new FastBuffer(length);
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/internal/buffer.mjs:1326-1331`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L1326-L1331)

```
Buffer.prototype.slice = function slice(start, end) {
  // Intentionally `this.subarray` so generic calls on a Uint8Array stay a
  // Uint8Array (Uint8Array.prototype.subarray) while Buffer instances pick up
  // Buffer.prototype.subarray and produce a FastBuffer.
  return this.subarray(start, end);
};
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/internal/buffer.mjs:1314-1324`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L1314-L1324)

```
Buffer.prototype.subarray = function subarray(start, end) {
  const srcLength = this.length;
  start = adjustOffset(start, srcLength);
  end = end !== undefined ? adjustOffset(end, srcLength) : srcLength;
  const newLength = end > start ? end - start : 0;
  return new FastBuffer(
    TypedArrayPrototypeGetBuffer(this),
    TypedArrayPrototypeGetByteOffset(this) + start,
    newLength,
  );
};
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/internal/buffer.mjs:258-271`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L258-L271)

```
function Buffer(arg, encodingOrOffset, length) {
  showFlaggedDeprecation();
  if (typeof arg === "number") {
    if (typeof encodingOrOffset === "string") {
      throw new codes.ERR_INVALID_ARG_TYPE(
        "string",
        "string",
        arg,
      );
    }
    return _allocUnsafe(arg);
  }
  return _from(arg, encodingOrOffset, length);
}
```
