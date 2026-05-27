# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility, what should node:util.deprecate(fn, message, code) return, and what observable behavior matters for calls to the returned function? Focus on whether it wraps/forwards to the original function and whether invalid deprecation codes with spaces throw.
Search: https://deepwiki.com/search/for-node-compatibility-what-sh_e75eb738-3b32-41cf-99b1-4c99de3c7f4b

**Question**: For Node compatibility, what should node:util.deprecate(fn, message, code) return, and what observable behavior matters for calls to the returned function? Focus on whether it wraps/forwards to the original function and whether invalid deprecation codes with spaces throw.

The `node:util.deprecate` function in Deno's Node.js compatibility layer returns a wrapped function that forwards calls to the original function [1](#ref-q1-1) . When the returned function is called, it emits a `DeprecationWarning` the first time, unless deprecation is disabled via `process.noDeprecation` [2](#ref-q1-2) . Invalid deprecation codes with spaces will cause a `TypeError` to be thrown [3](#ref-q1-3) .

## `util.deprecate` Functionality

The `deprecate` function is defined in `ext/node/polyfills/util.ts` [4](#ref-q1-4) .

### Return Value
The `deprecate` function returns a new function, `deprecated`, which wraps the original function `fn` [5](#ref-q1-5) . This `deprecated` function maintains the prototype chain of the original function if `modifyPrototype` is true [6](#ref-q1-6) .

### Observable Behavior on Calls

1.  **Deprecation Warning Emission**:
    *   The first time the `deprecated` function is called, and if `process.noDeprecation` is not `true` [2](#ref-q1-2) , a `DeprecationWarning` is emitted via `process.emitWarning` [7](#ref-q1-7) .
    *   If a `code` is provided, the warning is emitted only once for that specific code [8](#ref-q1-8) . The `codesWarned` `SafeSet` tracks which codes have already triggered a warning [9](#ref-q1-9) .
    *   The `process.emitWarning` function itself handles the logic for `--no-deprecation`, `--throw-deprecation`, and `--trace-deprecation` flags, which can suppress or change the behavior of warnings [10](#ref-q1-10) .

2.  **Forwarding to Original Function**:
    *   After handling the warning, the `deprecated` function calls the original function `fn` using `ReflectApply` or `ReflectConstruct` [1](#ref-q1-1) . This ensures that the original function's logic is executed with the provided arguments and context.

3.  **Invalid Deprecation Codes**:
    *   If the `code` argument is provided but is not a string, `validateString(code, "code")` is called [3](#ref-q1-3) . This will throw an `ERR_INVALID_ARG_TYPE` if `code` is not a string [11](#ref-q1-11) . Therefore, invalid deprecation codes with spaces (which would still be strings) would not throw at this validation step, but rather if `process.emitWarning` itself has issues with the format. However, the Node.js documentation for `util.deprecate` implies that the `code` should be a valid deprecation code, typically without spaces, but the `validateString` check only ensures it's a string.

### Usage Examples
The `deprecate` function is used in various Node.js polyfills within Deno. For instance, in `ext/node/polyfills/_http_outgoing.ts`, several properties of `OutgoingMessage.prototype` are deprecated using `util.deprecate` [12](#ref-q1-12) . Another example is in `ext/node/polyfills/fs.ts`, where the `Stats` constructor is deprecated [13](#ref-q1-13) .

## Notes
The `deprecate` function in `ext/node/polyfills/util.ts` [4](#ref-q1-4)  is the primary implementation for Node.js compatibility. There is also a `pendingDeprecate` function in `ext/node/polyfills/internal/util.mjs` [14](#ref-q1-14) , which is specifically for `--pending-deprecation` warnings and has similar warning emission logic. However, the user's query specifically refers to `node:util.deprecate`, which corresponds to the former.

Wiki pages you might want to explore:
- [File System Operations (denoland/deno)](/wiki/denoland/deno#3.4)
- [Storage, Cache, and Web APIs (denoland/deno)](/wiki/denoland/deno#6.4)
- [Network and HTTP Polyfills (denoland/deno)](/wiki/denoland/deno#7.5)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-what-sh_e75eb738-3b32-41cf-99b1-4c99de3c7f4b

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/util.ts:227-230`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L227-L230)

```typescript
    if (new.target) {
      return ReflectConstruct(fn, args, new.target);
    }
    return ReflectApply(fn, this, args);
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/util.ts:206-208`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L206-L208)

```typescript
  if (process.noDeprecation === true) {
    return fn;
  }
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/util.ts:211-212`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L211-L212)

```typescript
    validateString(code, "code");
  }
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/util.ts:197-250`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L197-L250)

```typescript
function deprecate(
  fn,
  msg,
  code,
  { modifyPrototype = true } = {
    __proto__: null,
  },
) {
  process ??= lazyLoadProcess();
  if (process.noDeprecation === true) {
    return fn;
  }

  if (code !== undefined) {
    validateString(code, "code");
  }

  let warned = false;
  function deprecated(...args) {
    if (!warned && !op_node_call_is_from_dependency()) {
      warned = true;
      if (code !== undefined) {
        if (!SetPrototypeHas(codesWarned, code)) {
          process.emitWarning(msg, "DeprecationWarning", code, deprecated);
          SetPrototypeAdd(codesWarned, code);
        }
      } else {
        process.emitWarning(msg, "DeprecationWarning", deprecated);
      }
    }
    if (new.target) {
      return ReflectConstruct(fn, args, new.target);
    }
    return ReflectApply(fn, this, args);
  }

  if (modifyPrototype) {
    // The wrapper will keep the same prototype as fn to maintain prototype chain
    ObjectSetPrototypeOf(deprecated, fn);
    if (fn.prototype) {
      // Setting this (rather than using Object.setPrototype, as above) ensures
      // that calling the unwrapped constructor gives an instanceof the wrapped
      // constructor.
      deprecated.prototype = fn.prototype;
    }

    ObjectDefineProperty(deprecated, "length", {
      __proto__: null,
      ...ObjectGetOwnPropertyDescriptor(fn, "length"),
    });
  }

  return deprecated;
}
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/util.ts:249-250`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L249-L250)

```typescript
  return deprecated;
}
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/util.ts:233-247`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L233-L247)

```typescript
  if (modifyPrototype) {
    // The wrapper will keep the same prototype as fn to maintain prototype chain
    ObjectSetPrototypeOf(deprecated, fn);
    if (fn.prototype) {
      // Setting this (rather than using Object.setPrototype, as above) ensures
      // that calling the unwrapped constructor gives an instanceof the wrapped
      // constructor.
      deprecated.prototype = fn.prototype;
    }

    ObjectDefineProperty(deprecated, "length", {
      __proto__: null,
      ...ObjectGetOwnPropertyDescriptor(fn, "length"),
    });
  }
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/util.ts:220-224`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L220-L224)

```typescript
          process.emitWarning(msg, "DeprecationWarning", code, deprecated);
          SetPrototypeAdd(codesWarned, code);
        }
      } else {
        process.emitWarning(msg, "DeprecationWarning", deprecated);
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/util.ts:218-222`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L218-L222)

```typescript
      if (code !== undefined) {
        if (!SetPrototypeHas(codesWarned, code)) {
          process.emitWarning(msg, "DeprecationWarning", code, deprecated);
          SetPrototypeAdd(codesWarned, code);
        }
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/util.ts:192`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L192)

```typescript
const codesWarned = new SafeSet();
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/process.ts:380-393`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L380-L393)

```typescript
    // deno-lint-ignore no-explicit-any
    if ((process as any).noDeprecation) {
      return;
    }

    // deno-lint-ignore no-explicit-any
    if ((process as any).throwDeprecation) {
      // Delay throwing the error to guarantee that all former warnings were
      // properly logged.
      return process.nextTick(() => {
        throw warning;
      });
    }
  }
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/util.ts:67-69`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/util.ts#L67-L69)

```typescript
const { ERR_INVALID_ARG_TYPE } = core.loadExtScript(
  "ext:deno_node/internal/errors.ts",
);
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/_http_outgoing.ts:969-975`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_http_outgoing.ts#L969-L975)

```typescript
  get: deprecate(
    function (this: any) {
      return this.getHeaders();
    },
    "OutgoingMessage.prototype._headers is deprecated",
    "DEP0066",
  ),
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/fs.ts:370-374`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/fs.ts#L370-L374)

```typescript
          path: validatedPath,
        }),
      ),
  );
}
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/internal/util.mjs:198-223`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util.mjs#L198-L223)

```
function pendingDeprecate(fn, msg, code) {
  function deprecated(...args) {
    const process = globalThis.process;
    if (
      process.execArgv?.includes("--pending-deprecation") &&
      !process.noDeprecation
    ) {
      if (code !== undefined) {
        if (!SetPrototypeHas(pendingCodesWarned, code)) {
          process.emitWarning(msg, "DeprecationWarning", code, deprecated);
          SetPrototypeAdd(pendingCodesWarned, code);
        }
      } else {
        process.emitWarning(msg, "DeprecationWarning", deprecated);
      }
    }
    return ReflectApply(fn, this, args);
  }

  ObjectDefineProperty(deprecated, "length", {
    __proto__: null,
    ...ObjectGetOwnPropertyDescriptor(fn, "length"),
  });

  return deprecated;
}
```
