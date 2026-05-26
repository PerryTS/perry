# DeepWiki Response

Repository: `denoland/deno`
Question: How does Deno's Node compatibility implementation of EventEmitter handle the 'removeListener' meta-event when removing a listener, especially for .off/.removeListener and .once wrappers? Please identify the relevant source files and tests showing that the emitted 'removeListener' event receives the event name and the original listener function identity.
Search: https://deepwiki.com/search/how-does-denos-node-compatibil_4baae612-088e-4515-b257-ef838d08ad9c

**Question**: How does Deno's Node compatibility implementation of EventEmitter handle the 'removeListener' meta-event when removing a listener, especially for .off/.removeListener and .once wrappers? Please identify the relevant source files and tests showing that the emitted 'removeListener' event receives the event name and the original listener function identity.

# Answer

You're asking about how Deno's Node.js compatibility layer for `EventEmitter` handles the `'removeListener'` meta-event when listeners are removed, particularly when using `.off()`, `.removeListener()`, and `.once()` wrappers, and how the original listener function identity is preserved.

## The `removeListener` Meta-Event Emission

The core implementation is in [1](#ref-q1-1) . When a listener is removed via `removeListener()`, the code checks if there's a `removeListener` event listener registered on the emitter itself. If so, it emits the `'removeListener'` meta-event with two arguments: the event name and the listener function.

The key logic appears in two places within the `removeListener` function:

1. **Single listener case** (lines 629-637): When the entire listener list is a single function, it emits `this.emit("removeListener", type, list.listener || listener)`. [2](#ref-q1-2) 

2. **Multiple listeners case** (lines 638-665): When there are multiple listeners in an array, after removing one, it emits `this.emit("removeListener", type, listener)`. [3](#ref-q1-3) 

## Handling `.once()` Wrappers

The critical detail for `.once()` is that when a listener is wrapped by `_onceWrap()`, the wrapper function stores a reference to the original listener: [4](#ref-q1-4) 

This `wrapped.listener = listener` property is essential. When removing a once-wrapped listener, the code checks `list.listener === listener` to match against the original function. This allows the `removeListener` event to emit the **original listener function identity**, not the wrapper.

Looking at the removal logic again: when checking if a listener matches, it uses `list[i] === listener || list[i].listener === listener` [5](#ref-q1-5) , which handles both direct listeners and wrapped listeners.

## `.off()` Alias

The `.off()` method is simply an alias: [6](#ref-q1-6) 

So it uses the exact same `removeListener` implementation.

## Test Coverage

The test file [7](#ref-q1-7)  contains basic EventEmitter tests, though the provided snippet doesn't show specific tests for the `removeListener` meta-event. However, the implementation itself demonstrates that:

1. The event name is passed as the first argument to the `removeListener` event
2. The listener function (or `list.listener` for wrapped functions) is passed as the second argument
3. This preserves the original listener identity even when using `.once()` wrappers



## Notes

The implementation correctly handles the Node.js EventEmitter semantics where the `'removeListener'` meta-event is emitted only if there are listeners registered for that event (checked via `events.removeListener !== undefined`). The wrapper pattern used by `.once()` is crucial—by storing the original listener on the wrapper function's `listener` property, Deno ensures that when you remove a once-wrapped listener, the `removeListener` event receives the original function reference, not the wrapper, matching Node.js behavior.

Wiki pages you might want to explore:
- [File System Operations (denoland/deno)](/wiki/denoland/deno#3.4)

View this search on DeepWiki: https://deepwiki.com/search/how-does-denos-node-compatibil_4baae612-088e-4515-b257-ef838d08ad9c

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/_events.mjs:613-668`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L613-L668)

```
EventEmitter.prototype.removeListener = function removeListener(
  type,
  listener,
) {
  checkListener(listener);

  const events = this._events;
  if (events === undefined) {
    return this;
  }

  const list = events[type];
  if (list === undefined) {
    return this;
  }

  if (list === listener || list.listener === listener) {
    if (--this._eventsCount === 0) {
      this._events = ObjectCreate(null);
    } else {
      delete events[type];
      if (events.removeListener) {
        this.emit("removeListener", type, list.listener || listener);
      }
    }
  } else if (typeof list !== "function") {
    let position = -1;

    for (let i = list.length - 1; i >= 0; i--) {
      if (list[i] === listener || list[i].listener === listener) {
        position = i;
        break;
      }
    }

    if (position < 0) {
      return this;
    }

    if (position === 0) {
      ArrayPrototypeShift(list);
    } else {
      spliceOne(list, position);
    }

    if (list.length === 1) {
      events[type] = list[0];
    }

    if (events.removeListener !== undefined) {
      this.emit("removeListener", type, listener);
    }
  }

  return this;
};
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/_events.mjs:629-636`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L629-L636)

```
  if (list === listener || list.listener === listener) {
    if (--this._eventsCount === 0) {
      this._events = ObjectCreate(null);
    } else {
      delete events[type];
      if (events.removeListener) {
        this.emit("removeListener", type, list.listener || listener);
      }
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/_events.mjs:662-664`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L662-L664)

```
    if (events.removeListener !== undefined) {
      this.emit("removeListener", type, listener);
    }
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/_events.mjs:571-574`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L571-L574)

```
  const wrapped = onceWrapper.bind(state);
  wrapped.listener = listener;
  state.wrapFn = wrapped;
  return wrapped;
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/_events.mjs:642`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L642)

```
      if (list[i] === listener || list[i].listener === listener) {
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/_events.mjs:670`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L670)

```
EventEmitter.prototype.off = EventEmitter.prototype.removeListener;
```

<a id="ref-q1-7"></a>
### [7] `tests/unit_node/events_test.ts:38-79`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/events_test.ts#L38-L79)

```typescript
Deno.test("addAbortListener", async () => {
  const { promise, resolve } = Promise.withResolvers<void>();
  const abortController = new AbortController();
  addAbortListener(abortController.signal, () => {
    resolve();
  });
  abortController.abort();
  await promise;
});

Deno.test("EventEmitter works when Object.create is deleted (#29929)", () => {
  const ObjectCreate = Object.create;
  Object.create = undefined!;
  try {
    const emitter = new EventEmitter();
    let called = false;
    emitter.on("foo", () => {
      called = true;
    });
    emitter.emit("foo");
    if (!called) throw new Error("Listener was not called");
  } finally {
    Object.create = ObjectCreate;
  }
});

Deno.test("EventEmitter works if Array.prototype.unshift is deleted", () => {
  const ArrayPrototypeUnshift = Array.prototype.unshift;
  // @ts-ignore -- this is fine for testing purposes
  delete Array.prototype.unshift;
  try {
    const emitter = new EventEmitter();
    let called = false;
    emitter.on("bar", () => {
      called = true;
    });
    emitter.emit("bar");
    if (!called) throw new Error("Listener was not called");
  } finally {
    Array.prototype.unshift = ArrayPrototypeUnshift;
  }
});
```
