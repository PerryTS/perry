# DeepWiki Response

Repository: `denoland/deno`
Question: In denoland/deno's Node compatibility layer, how do node:stream Readable/Writable instances implement EventEmitter listener bookkeeping for listenerCount() and listeners(), especially for data/end events? Identify the files and code paths that store listeners, count listeners, and return listener arrays or copies.
Search: https://deepwiki.com/search/in-denolanddenos-node-compatib_b4f9dfe6-e8fa-4bd2-b099-071f3a8657c9

**Question**: In denoland/deno's Node compatibility layer, how do node:stream Readable/Writable instances implement EventEmitter listener bookkeeping for listenerCount() and listeners(), especially for data/end events? Identify the files and code paths that store listeners, count listeners, and return listener arrays or copies.

In Deno's Node compatibility layer, `node:stream` `Readable`/`Writable` instances implement `EventEmitter` listener bookkeeping for `listenerCount()` and `listeners()` through the `_events` object and `_eventsCount` property on the `EventEmitter` prototype, defined in `ext/node/polyfills/_events.mjs` [1](#ref-q1-1) . The `Readable` stream specifically overrides the `on`, `addListener`, `removeListener`, and `removeAllListeners` methods to manage internal stream states related to `data` and `readable` events [2](#ref-q1-2) .

## Storing Listeners

Listeners are stored in the `_events` property of an `EventEmitter` instance [3](#ref-q1-3) . This property is an object where keys are event types (e.g., `"data"`, `"end"`) and values are either a single listener function or an array of listener functions [4](#ref-q1-4) . The `_eventsCount` property tracks the total number of distinct event types that have listeners [5](#ref-q1-5) .

When a listener is added via `addListener` or `on`, the `_addListener` function is called [6](#ref-q1-6) .
1.  If `_events` is `undefined`, it's initialized as an empty object [7](#ref-q1-7) .
2.  If no listeners exist for a given `type`, the `listener` is directly assigned to `events[type]` [8](#ref-q1-8) .
3.  If a single listener already exists, it's converted into an array, and the new listener is added [9](#ref-q1-9) .
4.  If an array of listeners already exists, the new listener is pushed to the array [10](#ref-q1-10) .

For `Readable` streams, the `on` method (aliased by `addListener`) also updates internal state flags like `kDataListening` and `kReadableListening` when `data` or `readable` events are registered [11](#ref-q1-11) .

## Counting Listeners

The `listenerCount()` method, implemented by `_listenerCount`, determines the number of listeners for a specific event `type` [12](#ref-q1-12) .
1.  It accesses the `_events` object [13](#ref-q1-13) .
2.  If `events[type]` is a single function, it returns 1 [14](#ref-q1-14) .
3.  If `events[type]` is an array, it returns the `length` of the array [15](#ref-q1-15) .
4.  If no listeners are found, it returns 0 [16](#ref-q1-16) .

The `Readable.prototype.removeListener` and `Readable.prototype.removeAllListeners` methods also interact with `listenerCount()` to manage the `kDataListening` state flag [17](#ref-q1-17) .

## Returning Listener Arrays

The `listeners()` method returns a copy of the array of listeners for a given event `type` [18](#ref-q1-18) . It calls the internal `_listeners` function with `unwrap` set to `true` [19](#ref-q1-19) .
1.  `_listeners` retrieves the `evlistener` for the specified `type` from `_events` [20](#ref-q1-20) .
2.  If `evlistener` is a single function, it returns an array containing that function (or its `listener` property if it's a wrapper) [21](#ref-q1-21) .
3.  If `evlistener` is an array, it calls `unwrapListeners` to create a new array where any wrapped listeners (e.g., from `once()`) are replaced by their original listener functions [22](#ref-q1-22) . The `unwrapListeners` function uses `arrayClone` to ensure a copy is returned [23](#ref-q1-23) .

The `rawListeners()` method is similar but calls `_listeners` with `unwrap` set to `false`, returning the listeners including any wrappers [24](#ref-q1-24) .

## Node.js Stream Specifics for `data`/`end` Events

For `node:stream` `Readable` instances, the `on` method has special logic for `data` and `readable` events [25](#ref-q1-25) .
-   When a `data` listener is added, the `kDataListening` flag is set in the `_readableState` [26](#ref-q1-26) . If the stream is not explicitly paused, it will `resume()` to start flowing data [27](#ref-q1-27) .
-   When a `readable` listener is added, the `kReadableListening` flag is set [28](#ref-q1-28) .
-   The `removeListener` and `removeAllListeners` methods for `Readable` streams also contain logic to update these internal state flags, particularly `kDataListening` when `data` listeners are removed [17](#ref-q1-17) .

## Notes

The `EventEmitter` implementation in Deno's Node compatibility layer is found in `ext/node/polyfills/_events.mjs` [29](#ref-q1-29) . The `Readable` stream's specific handling of listeners for `data` and `end` events is located in `ext/node/polyfills/internal/streams/readable.js` [2](#ref-q1-2) . The `EventEmitter` class itself provides the core mechanisms for listener storage, counting, and retrieval, which `Readable` and `Writable` streams inherit and extend.

Wiki pages you might want to explore:
- [File System Operations (denoland/deno)](/wiki/denoland/deno#3.4)

View this search on DeepWiki: https://deepwiki.com/search/in-denolanddenos-node-compatib_b4f9dfe6-e8fa-4bd2-b099-071f3a8657c9

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/_events.mjs:137-139`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L137-L139)

```
EventEmitter.prototype._events = undefined;
EventEmitter.prototype._eventsCount = 0;
EventEmitter.prototype._maxListeners = undefined;
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/internal/streams/readable.js:1210-1279`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1210-L1279)

```javascript
Readable.prototype.on = function (ev, fn) {
  const res = Stream.prototype.on.call(this, ev, fn);
  const state = this._readableState;

  if (ev === "data") {
    state[kState] |= kDataListening;

    // Update readableListening so that resume() may be a no-op
    // a few lines down. This is needed to support once('readable').
    state[kState] |= this.listenerCount("readable") > 0
      ? kReadableListening
      : 0;

    // Try start flowing on next tick if stream isn't explicitly paused.
    if ((state[kState] & (kHasFlowing | kFlowing)) !== kHasFlowing) {
      this.resume();
    }
  } else if (ev === "readable") {
    if ((state[kState] & (kEndEmitted | kReadableListening)) === 0) {
      state[kState] |= kReadableListening | kNeedReadable | kHasFlowing;
      state[kState] &= ~(kFlowing | kEmittedReadable);
      debug("on readable");
      if (state.length) {
        emitReadable(this);
      } else if ((state[kState] & kReading) === 0) {
        process.nextTick(nReadingNextTick, this);
      }
    }
  }

  return res;
};
Readable.prototype.addListener = Readable.prototype.on;

Readable.prototype.removeListener = function (ev, fn) {
  const state = this._readableState;

  const res = Stream.prototype.removeListener.call(this, ev, fn);

  if (ev === "readable") {
    // We need to check if there is someone still listening to
    // readable and reset the state. However this needs to happen
    // after readable has been emitted but before I/O (nextTick) to
    // support once('readable', fn) cycles. This means that calling
    // resume within the same tick will have no
    // effect.
    process.nextTick(updateReadableListening, this);
  } else if (ev === "data" && this.listenerCount("data") === 0) {
    state[kState] &= ~kDataListening;
  }

  return res;
};
Readable.prototype.off = Readable.prototype.removeListener;

Readable.prototype.removeAllListeners = function (ev) {
  const res = Stream.prototype.removeAllListeners.apply(this, arguments);

  if (ev === "readable" || ev === undefined) {
    // We need to check if there is someone still listening to
    // readable and reset the state. However this needs to happen
    // after readable has been emitted but before I/O (nextTick) to
    // support once('readable', fn) cycles. This means that calling
    // resume within the same tick will have no
    // effect.
    process.nextTick(updateReadableListening, this);
  }

  return res;
};
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/_events.mjs:475`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L475)

```
  events = target._events;
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/_events.mjs:492-507`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L492-L507)

```
  if (existing === undefined) {
    // Optimize the case of one listener. Don't need the extra array object.
    events[type] = listener;
    ++target._eventsCount;
  } else {
    if (typeof existing === "function") {
      // Adding the second element, need to change to array.
      existing = events[type] = prepend
        ? [listener, existing]
        : [existing, listener];
      // If we've already got an array, just append.
    } else if (prepend) {
      ArrayPrototypeUnshift(existing, listener);
    } else {
      ArrayPrototypePush(existing, listener);
    }
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/_events.mjs:138`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L138)

```
EventEmitter.prototype._eventsCount = 0;
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/_events.mjs:538-539`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L538-L539)

```
EventEmitter.prototype.addListener = function addListener(type, listener) {
  return _addListener(this, type, listener, false);
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/_events.mjs:476-477`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L476-L477)

```
  if (events === undefined) {
    events = target._events = ObjectCreate(null);
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/_events.mjs:492-495`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L492-L495)

```
  if (existing === undefined) {
    // Optimize the case of one listener. Don't need the extra array object.
    events[type] = listener;
    ++target._eventsCount;
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/_events.mjs:497-501`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L497-L501)

```
    if (typeof existing === "function") {
      // Adding the second element, need to change to array.
      existing = events[type] = prepend
        ? [listener, existing]
        : [existing, listener];
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/_events.mjs:503-506`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L503-L506)

```
    } else if (prepend) {
      ArrayPrototypeUnshift(existing, listener);
    } else {
      ArrayPrototypePush(existing, listener);
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/internal/streams/readable.js:1210-1242`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1210-L1242)

```javascript
Readable.prototype.on = function (ev, fn) {
  const res = Stream.prototype.on.call(this, ev, fn);
  const state = this._readableState;

  if (ev === "data") {
    state[kState] |= kDataListening;

    // Update readableListening so that resume() may be a no-op
    // a few lines down. This is needed to support once('readable').
    state[kState] |= this.listenerCount("readable") > 0
      ? kReadableListening
      : 0;

    // Try start flowing on next tick if stream isn't explicitly paused.
    if ((state[kState] & (kHasFlowing | kFlowing)) !== kHasFlowing) {
      this.resume();
    }
  } else if (ev === "readable") {
    if ((state[kState] & (kEndEmitted | kReadableListening)) === 0) {
      state[kState] |= kReadableListening | kNeedReadable | kHasFlowing;
      state[kState] &= ~(kFlowing | kEmittedReadable);
      debug("on readable");
      if (state.length) {
        emitReadable(this);
      } else if ((state[kState] & kReading) === 0) {
        process.nextTick(nReadingNextTick, this);
      }
    }
  }

  return res;
};
Readable.prototype.addListener = Readable.prototype.on;
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/_events.mjs:772`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L772)

```
const _listenerCount = function listenerCount(type, listener) {
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/_events.mjs:773`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L773)

```
  const events = this._events;
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/_events.mjs:778-784`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L778-L784)

```
    if (typeof evlistener === "function") {
      if (listener != null) {
        return listener === evlistener || listener === evlistener.listener
          ? 1
          : 0;
      }
      return 1;
```

<a id="ref-q1-15"></a>
### [15] `ext/node/polyfills/_events.mjs:785-799`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L785-L799)

```
    } else if (evlistener !== undefined) {
      if (listener != null) {
        let matching = 0;

        for (let i = 0, l = evlistener.length; i < l; i++) {
          if (
            evlistener[i] === listener || evlistener[i].listener === listener
          ) {
            matching++;
          }
        }

        return matching;
      }
      return evlistener.length;
```

<a id="ref-q1-16"></a>
### [16] `ext/node/polyfills/_events.mjs:801-803`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L801-L803)

```
  }

  return 0;
```

<a id="ref-q1-17"></a>
### [17] `ext/node/polyfills/internal/streams/readable.js:1244-1279`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1244-L1279)

```javascript
Readable.prototype.removeListener = function (ev, fn) {
  const state = this._readableState;

  const res = Stream.prototype.removeListener.call(this, ev, fn);

  if (ev === "readable") {
    // We need to check if there is someone still listening to
    // readable and reset the state. However this needs to happen
    // after readable has been emitted but before I/O (nextTick) to
    // support once('readable', fn) cycles. This means that calling
    // resume within the same tick will have no
    // effect.
    process.nextTick(updateReadableListening, this);
  } else if (ev === "data" && this.listenerCount("data") === 0) {
    state[kState] &= ~kDataListening;
  }

  return res;
};
Readable.prototype.off = Readable.prototype.removeListener;

Readable.prototype.removeAllListeners = function (ev) {
  const res = Stream.prototype.removeAllListeners.apply(this, arguments);

  if (ev === "readable" || ev === undefined) {
    // We need to check if there is someone still listening to
    // readable and reset the state. However this needs to happen
    // after readable has been emitted but before I/O (nextTick) to
    // support once('readable', fn) cycles. This means that calling
    // resume within the same tick will have no
    // effect.
    process.nextTick(updateReadableListening, this);
  }

  return res;
};
```

<a id="ref-q1-18"></a>
### [18] `ext/node/polyfills/_events.mjs:751-753`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L751-L753)

```
EventEmitter.prototype.listeners = function listeners(type) {
  return _listeners(this, type, true);
};
```

<a id="ref-q1-19"></a>
### [19] `ext/node/polyfills/_events.mjs:752`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L752)

```
  return _listeners(this, type, true);
```

<a id="ref-q1-20"></a>
### [20] `ext/node/polyfills/_events.mjs:727-733`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L727-L733)

```
  const events = target._events;

  if (events === undefined) {
    return [];
  }

  const evlistener = events[type];
```

<a id="ref-q1-21"></a>
### [21] `ext/node/polyfills/_events.mjs:738-739`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L738-L739)

```
  if (typeof evlistener === "function") {
    return unwrap ? [evlistener.listener || evlistener] : [evlistener];
```

<a id="ref-q1-22"></a>
### [22] `ext/node/polyfills/_events.mjs:742`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L742)

```
  return unwrap ? unwrapListeners(evlistener) : arrayClone(evlistener);
```

<a id="ref-q1-23"></a>
### [23] `ext/node/polyfills/_events.mjs:854-861`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L854-L861)

```
  const ret = arrayClone(arr);
  for (let i = 0; i < ret.length; ++i) {
    const orig = ret[i].listener;
    if (typeof orig === "function") {
      ret[i] = orig;
    }
  }
  return ret;
```

<a id="ref-q1-24"></a>
### [24] `ext/node/polyfills/_events.mjs:761-762`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L761-L762)

```
EventEmitter.prototype.rawListeners = function rawListeners(type) {
  return _listeners(this, type, false);
```

<a id="ref-q1-25"></a>
### [25] `ext/node/polyfills/internal/streams/readable.js:1210-1241`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1210-L1241)

```javascript
Readable.prototype.on = function (ev, fn) {
  const res = Stream.prototype.on.call(this, ev, fn);
  const state = this._readableState;

  if (ev === "data") {
    state[kState] |= kDataListening;

    // Update readableListening so that resume() may be a no-op
    // a few lines down. This is needed to support once('readable').
    state[kState] |= this.listenerCount("readable") > 0
      ? kReadableListening
      : 0;

    // Try start flowing on next tick if stream isn't explicitly paused.
    if ((state[kState] & (kHasFlowing | kFlowing)) !== kHasFlowing) {
      this.resume();
    }
  } else if (ev === "readable") {
    if ((state[kState] & (kEndEmitted | kReadableListening)) === 0) {
      state[kState] |= kReadableListening | kNeedReadable | kHasFlowing;
      state[kState] &= ~(kFlowing | kEmittedReadable);
      debug("on readable");
      if (state.length) {
        emitReadable(this);
      } else if ((state[kState] & kReading) === 0) {
        process.nextTick(nReadingNextTick, this);
      }
    }
  }

  return res;
};
```

<a id="ref-q1-26"></a>
### [26] `ext/node/polyfills/internal/streams/readable.js:1214`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1214)

```javascript
  if (ev === "data") {
```

<a id="ref-q1-27"></a>
### [27] `ext/node/polyfills/internal/streams/readable.js:1223-1225`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1223-L1225)

```javascript
    // Try start flowing on next tick if stream isn't explicitly paused.
    if ((state[kState] & (kHasFlowing | kFlowing)) !== kHasFlowing) {
      this.resume();
```

<a id="ref-q1-28"></a>
### [28] `ext/node/polyfills/internal/streams/readable.js:1228-1229`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1228-L1229)

```javascript
    if ((state[kState] & (kEndEmitted | kReadableListening)) === 0) {
      state[kState] |= kReadableListening | kNeedReadable | kHasFlowing;
```

<a id="ref-q1-29"></a>
### [29] `ext/node/polyfills/_events.mjs:99-101`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L99-L101)

```
function EventEmitter(opts) {
  FunctionPrototypeCall(EventEmitter.init, this, opts);
}
```
