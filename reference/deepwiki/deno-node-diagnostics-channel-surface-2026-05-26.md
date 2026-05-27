# DeepWiki Response

Repository: `denoland/deno`
Question: For Deno's Node compatibility implementation of node:diagnostics_channel, what module namespace surface is expected for deterministic probes such as hasSubscribers, channel, subscribe, unsubscribe, tracingChannel, Channel instance methods, TracingChannel methods, and experimental boundedChannel availability? Please cite the relevant source files that define or export these Node-compatible diagnostics_channel APIs and note how absent or unsupported exports should appear to namespace typeof probes.
Search: https://deepwiki.com/search/for-denos-node-compatibility-i_4c2e9130-b2d8-4696-a6b4-edfc599a0775

**Question**: For Deno's Node compatibility implementation of node:diagnostics_channel, what module namespace surface is expected for deterministic probes such as hasSubscribers, channel, subscribe, unsubscribe, tracingChannel, Channel instance methods, TracingChannel methods, and experimental boundedChannel availability? Please cite the relevant source files that define or export these Node-compatible diagnostics_channel APIs and note how absent or unsupported exports should appear to namespace typeof probes.

Deno's Node compatibility layer for `node:diagnostics_channel` exposes several deterministic probes and methods. The core implementation is found in `ext/node/polyfills/diagnostics_channel.js` [1](#ref-q1-1)  and `ext/node/polyfills/diagnostics_channel_esm.js` [2](#ref-q1-2) , with type definitions in `cli/tsc/dts/node/diagnostics_channel.d.cts` [3](#ref-q1-3) .

## Module Namespace Surface

The `node:diagnostics_channel` module exports the following functions and classes:

*   `channel(name)`: Returns a `Channel` instance for the given name. [4](#ref-q1-4)
*   `hasSubscribers(name)`: Checks if a channel has active subscribers. [5](#ref-q1-5)
*   `subscribe(name, subscription)`: Subscribes a handler to a named channel. [6](#ref-q1-6)
*   `unsubscribe(name, subscription)`: Unsubscribes a handler from a named channel. [7](#ref-q1-7)
*   `tracingChannel(nameOrChannels)`: Creates a `TracingChannel` wrapper. [8](#ref-q1-8)
*   `Channel`: The class representing a diagnostics channel. [9](#ref-q1-9)

These are explicitly exported in `ext/node/polyfills/diagnostics_channel_esm.js` [10](#ref-q1-10)  for ESM imports and as the default export in `ext/node/polyfills/diagnostics_channel.js` [11](#ref-q1-11)  for CommonJS compatibility.

### `Channel` Instance Methods

A `Channel` instance, obtained via `diagnostics_channel.channel(name)`, exposes the following methods and properties:

*   `name`: A read-only property returning the channel's name. [12](#ref-q1-12)
*   `hasSubscribers`: A getter that returns `true` if there are active subscribers, `false` otherwise. [13](#ref-q1-13)
*   `publish(message)`: Publishes a message to all subscribers of the channel. [14](#ref-q1-14)
*   `subscribe(onMessage)`: Subscribes a handler to this specific channel instance. [15](#ref-q1-15)
*   `unsubscribe(onMessage)`: Unsubscribes a handler from this specific channel instance. [16](#ref-q1-16)
*   `bindStore(store, transform)`: Binds an `AsyncLocalStorage` store to the channel. [17](#ref-q1-17)
*   `unbindStore(store)`: Unbinds an `AsyncLocalStorage` store from the channel. [18](#ref-q1-18)
*   `runStores(data, fn, thisArg, ...args)`: Runs a function within the context of bound stores. [19](#ref-q1-19)

### `TracingChannel` Methods

The `TracingChannel` class, created by `diagnostics_channel.tracingChannel()`, provides methods for tracing synchronous, promise-based, and callback-based operations:

*   `hasSubscribers`: A getter that returns `true` if any of its constituent channels (`start`, `end`, `asyncStart`, `asyncEnd`, `error`) have subscribers. [20](#ref-q1-20)
*   `subscribe(handlers)`: Subscribes handlers to the tracing channel's individual event channels. [21](#ref-q1-21)
*   `unsubscribe(handlers)`: Unsubscribes handlers from the tracing channel's individual event channels. [22](#ref-q1-22)
*   `traceSync(fn, context, thisArg, ...args)`: Traces a synchronous function call. [23](#ref-q1-23)
*   `tracePromise(fn, context, thisArg, ...args)`: Traces a promise-returning function call. [24](#ref-q1-24)
*   `traceCallback(fn, position, context, thisArg, ...args)`: Traces a callback-receiving function call. [25](#ref-q1-25)

### Experimental `boundedChannel` Availability

The `boundedChannel` API is not implemented in Deno's `node:diagnostics_channel` compatibility layer. Therefore, a `typeof` probe for `diagnostics_channel.boundedChannel` would return `'undefined'`. [26](#ref-q1-26)

## Absent or Unsupported Exports

For any `node:diagnostics_channel` export that is absent or unsupported in Deno's compatibility layer, a `typeof` probe would deterministically return `'undefined'`. This is the standard JavaScript behavior for accessing non-existent properties on an object or module namespace. [26](#ref-q1-26)

## Notes

The `node:diagnostics_channel` implementation in Deno is located in `ext/node/polyfills/diagnostics_channel.js` [26](#ref-q1-26) . The `WeakRefMap` class is used internally to manage channels, allowing them to be garbage collected when no longer referenced. [27](#ref-q1-27)  The `ActiveChannel` class is a specialized prototype used when a channel has active subscribers or bound stores, optimizing performance by avoiding checks for subscribers when none are present. [28](#ref-q1-28)  The `tests/node_compat/config.jsonc` file lists various Node.js compatibility tests, including several for `diagnostics_channel`, indicating active testing and compatibility efforts. [29](#ref-q1-29)

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/for-denos-node-compatibility-i_4c2e9130-b2d8-4696-a6b4-edfc599a0775

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/diagnostics_channel.js:1-336`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L1-L336)

```javascript
// Copyright 2018-2026 the Deno authors. MIT license.
// Copyright Joyent and Node contributors. All rights reserved. MIT license.

// TODO(petamoriken): enable prefer-primordials for node polyfills
// deno-lint-ignore-file prefer-primordials ban-untagged-todo

(function () {
const { core, primordials } = globalThis.__bootstrap;
const { ERR_INVALID_ARG_TYPE } = core.loadExtScript(
  "ext:deno_node/internal/errors.ts",
);
const { nextTick } = core.loadExtScript("ext:deno_node/_next_tick.ts");
const { validateFunction } = core.loadExtScript(
  "ext:deno_node/internal/validators.mjs",
);

const {
  ArrayPrototypeAt,
  ArrayPrototypeIndexOf,
  ArrayPrototypePush,
  ArrayPrototypeSplice,
  ObjectDefineProperty,
  ObjectGetPrototypeOf,
  ObjectSetPrototypeOf,
  Promise,
  PromisePrototypeThen,
  PromiseReject,
  PromiseResolve,
  ReflectApply,
  SafeFinalizationRegistry,
  SafeMap,
  SymbolHasInstance,
} = primordials;
const { WeakReference } = core.loadExtScript("ext:deno_node/internal/util.mjs");

// Can't delete when weakref count reaches 0 as it could increment again.
// Only GC can be used as a valid time to clean up the channels map.
class WeakRefMap extends SafeMap {
  #finalizers = new SafeFinalizationRegistry((key) => {
    this.delete(key);
  });

  set(key, value) {
    this.#finalizers.register(value, key);
    return super.set(key, new WeakReference(value));
  }

  get(key) {
    return super.get(key)?.get();
  }

  incRef(key) {
    return super.get(key)?.incRef();
  }

  decRef(key) {
    return super.get(key)?.decRef();
  }
}

function markActive(channel) {
  ObjectSetPrototypeOf(channel, ActiveChannel.prototype);
  channel._subscribers = [];
  channel._stores = new SafeMap();
}

function maybeMarkInactive(channel) {
  // When there are no more active subscribers or bound, restore to fast prototype.
  if (!channel._subscribers.length && !channel._stores.size) {
    ObjectSetPrototypeOf(channel, Channel.prototype);
    channel._subscribers = undefined;
    channel._stores = undefined;
  }
}

function defaultTransform(data) {
  return data;
}

function wrapStoreRun(store, data, next, transform = defaultTransform) {
  return () => {
    let context;
    try {
      context = transform(data);
    } catch (err) {
      nextTick(() => {
        // TODO(bartlomieju): in Node.js this is using `triggerUncaughtException` API, need
        // to clarify if we need that or if just throwing the error is enough here.
        throw err;
        // triggerUncaughtException(err, false);
      });
      return next();
    }

    return store.run(context, next);
  };
}

class ActiveChannel {
  subscribe(subscription) {
    validateFunction(subscription, "subscription");
    ArrayPrototypePush(this._subscribers, subscription);
    channels.incRef(this.name);
  }

  unsubscribe(subscription) {
    const index = ArrayPrototypeIndexOf(this._subscribers, subscription);
    if (index === -1) return false;

    ArrayPrototypeSplice(this._subscribers, index, 1);

    channels.decRef(this.name);
    maybeMarkInactive(this);

    return true;
  }

  bindStore(store, transform) {
    const replacing = this._stores.has(store);
    if (!replacing) channels.incRef(this.name);
    this._stores.set(store, transform);
  }

  unbindStore(store) {
    if (!this._stores.has(store)) {
      return false;
    }

    this._stores.delete(store);

    channels.decRef(this.name);
    maybeMarkInactive(this);

    return true;
  }

  get hasSubscribers() {
    return true;
  }

  publish(data) {
    for (let i = 0; i < (this._subscribers?.length || 0); i++) {
      try {
        const onMessage = this._subscribers[i];
        onMessage(data, this.name);
      } catch (err) {
        nextTick(() => {
          // TODO(bartlomieju): in Node.js this is using `triggerUncaughtException` API, need
          // to clarify if we need that or if just throwing the error is enough here.
          throw err;
          // triggerUncaughtException(err, false);
        });
      }
    }
  }

  runStores(data, fn, thisArg, ...args) {
    let run = () => {
      this.publish(data);
      return ReflectApply(fn, thisArg, args);
    };

    for (const entry of this._stores.entries()) {
      const store = entry[0];
      const transform = entry[1];
      run = wrapStoreRun(store, data, run, transform);
    }

    return run();
  }
}

class Channel {
  constructor(name) {
    this._subscribers = undefined;
    this._stores = undefined;
    this.name = name;

    channels.set(name, this);
  }

  static [SymbolHasInstance](instance) {
    const prototype = ObjectGetPrototypeOf(instance);
    return prototype === Channel.prototype ||
      prototype === ActiveChannel.prototype;
  }

  subscribe(subscription) {
    markActive(this);
    this.subscribe(subscription);
  }

  unsubscribe() {
    return false;
  }

  bindStore(store, transform) {
    markActive(this);
    this.bindStore(store, transform);
  }

  unbindStore() {
    return false;
  }

  get hasSubscribers() {
    return false;
  }

  publish() {}

  runStores(_data, fn, thisArg, ...args) {
    return ReflectApply(fn, thisArg, args);
  }
}

const channels = new WeakRefMap();

function channel(name) {
  const ch = channels.get(name);
  if (ch) return ch;

  if (typeof name !== "string" && typeof name !== "symbol") {
    throw new ERR_INVALID_ARG_TYPE("channel", ["string", "symbol"], name);
  }

  return new Channel(name);
}

function subscribe(name, subscription) {
  return channel(name).subscribe(subscription);
}

function unsubscribe(name, subscription) {
  return channel(name).unsubscribe(subscription);
}

function hasSubscribers(name) {
  const ch = channels.get(name);
  if (!ch) return false;

  return ch.hasSubscribers;
}

const traceEvents = [
  "start",
  "end",
  "asyncStart",
  "asyncEnd",
  "error",
];

function assertChannel(value, name) {
  if (!(value instanceof Channel)) {
    throw new ERR_INVALID_ARG_TYPE(name, ["Channel"], value);
  }
}

function tracingChannelFrom(nameOrChannels, name) {
  if (typeof nameOrChannels === "string") {
    return channel(`tracing:${nameOrChannels}:${name}`);
  }

  if (typeof nameOrChannels === "object" && nameOrChannels !== null) {
    const ch = nameOrChannels[name];
    assertChannel(ch, `nameOrChannels.${name}`);
    return ch;
  }

  throw new ERR_INVALID_ARG_TYPE("nameOrChannels", [
    "string",
    "object",
    "Channel",
  ], nameOrChannels);
}

class TracingChannel {
  constructor(nameOrChannels) {
    for (const eventName of traceEvents) {
      ObjectDefineProperty(this, eventName, {
        __proto__: null,
        value: tracingChannelFrom(nameOrChannels, eventName),
      });
    }
  }

  get hasSubscribers() {
    return this.start.hasSubscribers ||
      this.end.hasSubscribers ||
      this.asyncStart.hasSubscribers ||
      this.asyncEnd.hasSubscribers ||
      this.error.hasSubscribers;
  }

  subscribe(handlers) {
    for (const name of traceEvents) {
      if (!handlers[name]) continue;

      this[name]?.subscribe(handlers[name]);
    }
  }

  unsubscribe(handlers) {
    let done = true;

    for (const name of traceEvents) {
      if (!handlers[name]) continue;

      if (!this[name]?.unsubscribe(handlers[name])) {
        done = false;
      }
    }

    return done;
  }

  traceSync(fn, context = {}, thisArg, ...args) {
    if (!this.hasSubscribers) {
      return ReflectApply(fn, thisArg, args);
    }

    const { start, end, error } = this;

    return start.runStores(context, () => {
      try {
        const result = ReflectApply(fn, thisArg, args);
        context.result = result;
        return result;
      } catch (err) {
        context.error = err;
        error.publish(context);
        throw err;
      } finally {
        end.publish(context);
      }
    });
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/diagnostics_channel_esm.js:1-16`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel_esm.js#L1-L16)

<a id="ref-q1-3"></a>
### [3] `cli/tsc/dts/node/diagnostics_channel.d.cts:1-240`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/diagnostics_channel.d.cts#L1-L240)

```
/**
 * The `node:diagnostics_channel` module provides an API to create named channels
 * to report arbitrary message data for diagnostics purposes.
 *
 * It can be accessed using:
 *
 * ```js
 * import diagnostics_channel from 'node:diagnostics_channel';
 * ```
 *
 * It is intended that a module writer wanting to report diagnostics messages
 * will create one or many top-level channels to report messages through.
 * Channels may also be acquired at runtime but it is not encouraged
 * due to the additional overhead of doing so. Channels may be exported for
 * convenience, but as long as the name is known it can be acquired anywhere.
 *
 * If you intend for your module to produce diagnostics data for others to
 * consume it is recommended that you include documentation of what named
 * channels are used along with the shape of the message data. Channel names
 * should generally include the module name to avoid collisions with data from
 * other modules.
 * @since v15.1.0, v14.17.0
 * @see [source](https://github.com/nodejs/node/blob/v24.x/lib/diagnostics_channel.js)
 */
declare module "diagnostics_channel" {
    import { AsyncLocalStorage } from "node:async_hooks";
    /**
     * Check if there are active subscribers to the named channel. This is helpful if
     * the message you want to send might be expensive to prepare.
     *
     * This API is optional but helpful when trying to publish messages from very
     * performance-sensitive code.
     *
     * ```js
     * import diagnostics_channel from 'node:diagnostics_channel';
     *
     * if (diagnostics_channel.hasSubscribers('my-channel')) {
     *   // There are subscribers, prepare and publish message
     * }
     * ```
     * @since v15.1.0, v14.17.0
     * @param name The channel name
     * @return If there are active subscribers
     */
    function hasSubscribers(name: string | symbol): boolean;
    /**
     * This is the primary entry-point for anyone wanting to publish to a named
     * channel. It produces a channel object which is optimized to reduce overhead at
     * publish time as much as possible.
     *
     * ```js
     * import diagnostics_channel from 'node:diagnostics_channel';
     *
     * const channel = diagnostics_channel.channel('my-channel');
     * ```
     * @since v15.1.0, v14.17.0
     * @param name The channel name
     * @return The named channel object
     */
    function channel(name: string | symbol): Channel;
    type ChannelListener = (message: unknown, name: string | symbol) => void;
    /**
     * Register a message handler to subscribe to this channel. This message handler
     * will be run synchronously whenever a message is published to the channel. Any
     * errors thrown in the message handler will trigger an `'uncaughtException'`.
     *
     * ```js
     * import diagnostics_channel from 'node:diagnostics_channel';
     *
     * diagnostics_channel.subscribe('my-channel', (message, name) => {
     *   // Received data
     * });
     * ```
     * @since v18.7.0, v16.17.0
     * @param name The channel name
     * @param onMessage The handler to receive channel messages
     */
    function subscribe(name: string | symbol, onMessage: ChannelListener): void;
    /**
     * Remove a message handler previously registered to this channel with {@link subscribe}.
     *
     * ```js
     * import diagnostics_channel from 'node:diagnostics_channel';
     *
     * function onMessage(message, name) {
     *   // Received data
     * }
     *
     * diagnostics_channel.subscribe('my-channel', onMessage);
     *
     * diagnostics_channel.unsubscribe('my-channel', onMessage);
     * ```
     * @since v18.7.0, v16.17.0
     * @param name The channel name
     * @param onMessage The previous subscribed handler to remove
     * @return `true` if the handler was found, `false` otherwise.
     */
    function unsubscribe(name: string | symbol, onMessage: ChannelListener): boolean;
    /**
     * Creates a `TracingChannel` wrapper for the given `TracingChannel Channels`. If a name is given, the corresponding tracing
     * channels will be created in the form of `tracing:${name}:${eventType}` where `eventType` corresponds to the types of `TracingChannel Channels`.
     *
     * ```js
     * import diagnostics_channel from 'node:diagnostics_channel';
     *
     * const channelsByName = diagnostics_channel.tracingChannel('my-channel');
     *
     * // or...
     *
     * const channelsByCollection = diagnostics_channel.tracingChannel({
     *   start: diagnostics_channel.channel('tracing:my-channel:start'),
     *   end: diagnostics_channel.channel('tracing:my-channel:end'),
     *   asyncStart: diagnostics_channel.channel('tracing:my-channel:asyncStart'),
     *   asyncEnd: diagnostics_channel.channel('tracing:my-channel:asyncEnd'),
     *   error: diagnostics_channel.channel('tracing:my-channel:error'),
     * });
     * ```
     * @since v19.9.0
     * @experimental
     * @param nameOrChannels Channel name or object containing all the `TracingChannel Channels`
     * @return Collection of channels to trace with
     */
    function tracingChannel<
        StoreType = unknown,
        ContextType extends object = StoreType extends object ? StoreType : object,
    >(
        nameOrChannels: string | TracingChannelCollection<StoreType, ContextType>,
    ): TracingChannel<StoreType, ContextType>;
    /**
     * The class `Channel` represents an individual named channel within the data
     * pipeline. It is used to track subscribers and to publish messages when there
     * are subscribers present. It exists as a separate object to avoid channel
     * lookups at publish time, enabling very fast publish speeds and allowing
     * for heavy use while incurring very minimal cost. Channels are created with {@link channel}, constructing a channel directly
     * with `new Channel(name)` is not supported.
     * @since v15.1.0, v14.17.0
     */
    class Channel<StoreType = unknown, ContextType = StoreType> {
        readonly name: string | symbol;
        /**
         * Check if there are active subscribers to this channel. This is helpful if
         * the message you want to send might be expensive to prepare.
         *
         * This API is optional but helpful when trying to publish messages from very
         * performance-sensitive code.
         *
         * ```js
         * import diagnostics_channel from 'node:diagnostics_channel';
         *
         * const channel = diagnostics_channel.channel('my-channel');
         *
         * if (channel.hasSubscribers) {
         *   // There are subscribers, prepare and publish message
         * }
         * ```
         * @since v15.1.0, v14.17.0
         */
        readonly hasSubscribers: boolean;
        private constructor(name: string | symbol);
        /**
         * Publish a message to any subscribers to the channel. This will trigger
         * message handlers synchronously so they will execute within the same context.
         *
         * ```js
         * import diagnostics_channel from 'node:diagnostics_channel';
         *
         * const channel = diagnostics_channel.channel('my-channel');
         *
         * channel.publish({
         *   some: 'message',
         * });
         * ```
         * @since v15.1.0, v14.17.0
         * @param message The message to send to the channel subscribers
         */
        publish(message: unknown): void;
        /**
         * Register a message handler to subscribe to this channel. This message handler
         * will be run synchronously whenever a message is published to the channel. Any
         * errors thrown in the message handler will trigger an `'uncaughtException'`.
         *
         * ```js
         * import diagnostics_channel from 'node:diagnostics_channel';
         *
         * const channel = diagnostics_channel.channel('my-channel');
         *
         * channel.subscribe((message, name) => {
         *   // Received data
         * });
         * ```
         * @since v15.1.0, v14.17.0
         * @deprecated Since v18.7.0,v16.17.0 - Use {@link subscribe(name, onMessage)}
         * @param onMessage The handler to receive channel messages
         */
        subscribe(onMessage: ChannelListener): void;
        /**
         * Remove a message handler previously registered to this channel with `channel.subscribe(onMessage)`.
         *
         * ```js
         * import diagnostics_channel from 'node:diagnostics_channel';
         *
         * const channel = diagnostics_channel.channel('my-channel');
         *
         * function onMessage(message, name) {
         *   // Received data
         * }
         *
         * channel.subscribe(onMessage);
         *
         * channel.unsubscribe(onMessage);
         * ```
         * @since v15.1.0, v14.17.0
         * @deprecated Since v18.7.0,v16.17.0 - Use {@link unsubscribe(name, onMessage)}
         * @param onMessage The previous subscribed handler to remove
         * @return `true` if the handler was found, `false` otherwise.
         */
        unsubscribe(onMessage: ChannelListener): void;
        /**
         * When `channel.runStores(context, ...)` is called, the given context data
         * will be applied to any store bound to the channel. If the store has already been
         * bound the previous `transform` function will be replaced with the new one.
         * The `transform` function may be omitted to set the given context data as the
         * context directly.
         *
         * ```js
         * import diagnostics_channel from 'node:diagnostics_channel';
         * import { AsyncLocalStorage } from 'node:async_hooks';
         *
         * const store = new AsyncLocalStorage();
         *
         * const channel = diagnostics_channel.channel('my-channel');
         *
         * channel.bindStore(store, (data) => {
         *   return { data };
         * });
         * ```
         * @since v19.9.0
         * @experimental
         * @param store The store to which to bind the context data
         * @param transform Transform context data before setting the store context
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/diagnostics_channel.js:219-228`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L219-L228)

```javascript
function channel(name) {
  const ch = channels.get(name);
  if (ch) return ch;

  if (typeof name !== "string" && typeof name !== "symbol") {
    throw new ERR_INVALID_ARG_TYPE("channel", ["string", "symbol"], name);
  }

  return new Channel(name);
}
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/diagnostics_channel.js:238-243`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L238-L243)

```javascript
function hasSubscribers(name) {
  const ch = channels.get(name);
  if (!ch) return false;

  return ch.hasSubscribers;
}
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/diagnostics_channel.js:230-232`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L230-L232)

```javascript
function subscribe(name, subscription) {
  return channel(name).subscribe(subscription);
}
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/diagnostics_channel.js:234-236`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L234-L236)

```javascript
function unsubscribe(name, subscription) {
  return channel(name).unsubscribe(subscription);
}
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/diagnostics_channel.js:424-426`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L424-L426)

```javascript
function tracingChannel(nameOrChannels) {
  return new TracingChannel(nameOrChannels);
}
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/diagnostics_channel.js:173-215`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L173-L215)

```javascript
class Channel {
  constructor(name) {
    this._subscribers = undefined;
    this._stores = undefined;
    this.name = name;

    channels.set(name, this);
  }

  static [SymbolHasInstance](instance) {
    const prototype = ObjectGetPrototypeOf(instance);
    return prototype === Channel.prototype ||
      prototype === ActiveChannel.prototype;
  }

  subscribe(subscription) {
    markActive(this);
    this.subscribe(subscription);
  }

  unsubscribe() {
    return false;
  }

  bindStore(store, transform) {
    markActive(this);
    this.bindStore(store, transform);
  }

  unbindStore() {
    return false;
  }

  get hasSubscribers() {
    return false;
  }

  publish() {}

  runStores(_data, fn, thisArg, ...args) {
    return ReflectApply(fn, thisArg, args);
  }
}
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/diagnostics_channel_esm.js:5-12`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel_esm.js#L5-L12)

```javascript
export const {
  channel,
  hasSubscribers,
  subscribe,
  tracingChannel,
  unsubscribe,
  Channel,
} = mod;
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/diagnostics_channel.js:429-436`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L429-L436)

```javascript
  default: {
    channel,
    hasSubscribers,
    subscribe,
    tracingChannel,
    unsubscribe,
    Channel,
  },
```

<a id="ref-q1-12"></a>
### [12] `cli/tsc/dts/node/diagnostics_channel.d.cts:139`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/diagnostics_channel.d.cts#L139)

```
        readonly name: string | symbol;
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/diagnostics_channel.js:206-208`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L206-L208)

```javascript
  get hasSubscribers() {
    return false;
  }
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/diagnostics_channel.js:141-155`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L141-L155)

```javascript
  publish(data) {
    for (let i = 0; i < (this._subscribers?.length || 0); i++) {
      try {
        const onMessage = this._subscribers[i];
        onMessage(data, this.name);
      } catch (err) {
        nextTick(() => {
          // TODO(bartlomieju): in Node.js this is using `triggerUncaughtException` API, need
          // to clarify if we need that or if just throwing the error is enough here.
          throw err;
          // triggerUncaughtException(err, false);
        });
      }
    }
  }
```

<a id="ref-q1-15"></a>
### [15] `ext/node/polyfills/diagnostics_channel.js:188-191`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L188-L191)

```javascript
  subscribe(subscription) {
    markActive(this);
    this.subscribe(subscription);
  }
```

<a id="ref-q1-16"></a>
### [16] `ext/node/polyfills/diagnostics_channel.js:193-195`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L193-L195)

```javascript
  unsubscribe() {
    return false;
  }
```

<a id="ref-q1-17"></a>
### [17] `ext/node/polyfills/diagnostics_channel.js:118-122`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L118-L122)

```javascript
  bindStore(store, transform) {
    const replacing = this._stores.has(store);
    if (!replacing) channels.incRef(this.name);
    this._stores.set(store, transform);
  }
```

<a id="ref-q1-18"></a>
### [18] `ext/node/polyfills/diagnostics_channel.js:124-135`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L124-L135)

```javascript
  unbindStore(store) {
    if (!this._stores.has(store)) {
      return false;
    }

    this._stores.delete(store);

    channels.decRef(this.name);
    maybeMarkInactive(this);

    return true;
  }
```

<a id="ref-q1-19"></a>
### [19] `ext/node/polyfills/diagnostics_channel.js:157-170`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L157-L170)

```javascript
  runStores(data, fn, thisArg, ...args) {
    let run = () => {
      this.publish(data);
      return ReflectApply(fn, thisArg, args);
    };

    for (const entry of this._stores.entries()) {
      const store = entry[0];
      const transform = entry[1];
      run = wrapStoreRun(store, data, run, transform);
    }

    return run();
  }
```

<a id="ref-q1-20"></a>
### [20] `ext/node/polyfills/diagnostics_channel.js:287-293`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L287-L293)

```javascript
  get hasSubscribers() {
    return this.start.hasSubscribers ||
      this.end.hasSubscribers ||
      this.asyncStart.hasSubscribers ||
      this.asyncEnd.hasSubscribers ||
      this.error.hasSubscribers;
  }
```

<a id="ref-q1-21"></a>
### [21] `ext/node/polyfills/diagnostics_channel.js:295-301`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L295-L301)

```javascript
  subscribe(handlers) {
    for (const name of traceEvents) {
      if (!handlers[name]) continue;

      this[name]?.subscribe(handlers[name]);
    }
  }
```

<a id="ref-q1-22"></a>
### [22] `ext/node/polyfills/diagnostics_channel.js:303-315`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L303-L315)

```javascript
  unsubscribe(handlers) {
    let done = true;

    for (const name of traceEvents) {
      if (!handlers[name]) continue;

      if (!this[name]?.unsubscribe(handlers[name])) {
        done = false;
      }
    }

    return done;
  }
```

<a id="ref-q1-23"></a>
### [23] `ext/node/polyfills/diagnostics_channel.js:317-336`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L317-L336)

```javascript
  traceSync(fn, context = {}, thisArg, ...args) {
    if (!this.hasSubscribers) {
      return ReflectApply(fn, thisArg, args);
    }

    const { start, end, error } = this;

    return start.runStores(context, () => {
      try {
        const result = ReflectApply(fn, thisArg, args);
        context.result = result;
        return result;
      } catch (err) {
        context.error = err;
        error.publish(context);
        throw err;
      } finally {
        end.publish(context);
      }
    });
```

<a id="ref-q1-24"></a>
### [24] `cli/tsc/dts/node/diagnostics_channel.d.cts:484-489`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/diagnostics_channel.d.cts#L484-L489)

```
        tracePromise<ThisArg = any, Args extends any[] = any[], Result = any>(
            fn: (this: ThisArg, ...args: Args) => Promise<Result>,
            context?: ContextType,
            thisArg?: ThisArg,
            ...args: Args
        ): Promise<Result>;
```

<a id="ref-q1-25"></a>
### [25] `ext/node/polyfills/diagnostics_channel.js:381-421`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L381-L421)

```javascript
  traceCallback(fn, position = -1, context = {}, thisArg, ...args) {
    if (!this.hasSubscribers) {
      return ReflectApply(fn, thisArg, args);
    }

    const { start, end, asyncStart, asyncEnd, error } = this;

    function wrappedCallback(err, res) {
      if (err) {
        context.error = err;
        error.publish(context);
      } else {
        context.result = res;
      }

      // Using runStores here enables manual context failure recovery
      asyncStart.runStores(context, () => {
        try {
          return ReflectApply(callback, this, arguments);
        } finally {
          asyncEnd.publish(context);
        }
      });
    }

    const callback = ArrayPrototypeAt(args, position);
    validateFunction(callback, "callback");
    ArrayPrototypeSplice(args, position, 1, wrappedCallback);

    return start.runStores(context, () => {
      try {
        return ReflectApply(fn, thisArg, args);
      } catch (err) {
        context.error = err;
        error.publish(context);
        throw err;
      } finally {
        end.publish(context);
      }
    });
  }
```

<a id="ref-q1-26"></a>
### [26] `ext/node/polyfills/diagnostics_channel.js:1-445`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L1-L445)

<a id="ref-q1-27"></a>
### [27] `ext/node/polyfills/diagnostics_channel.js:38-59`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L38-L59)

```javascript
class WeakRefMap extends SafeMap {
  #finalizers = new SafeFinalizationRegistry((key) => {
    this.delete(key);
  });

  set(key, value) {
    this.#finalizers.register(value, key);
    return super.set(key, new WeakReference(value));
  }

  get(key) {
    return super.get(key)?.get();
  }

  incRef(key) {
    return super.get(key)?.incRef();
  }

  decRef(key) {
    return super.get(key)?.decRef();
  }
}
```

<a id="ref-q1-28"></a>
### [28] `ext/node/polyfills/diagnostics_channel.js:99-170`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/diagnostics_channel.js#L99-L170)

```javascript
class ActiveChannel {
  subscribe(subscription) {
    validateFunction(subscription, "subscription");
    ArrayPrototypePush(this._subscribers, subscription);
    channels.incRef(this.name);
  }

  unsubscribe(subscription) {
    const index = ArrayPrototypeIndexOf(this._subscribers, subscription);
    if (index === -1) return false;

    ArrayPrototypeSplice(this._subscribers, index, 1);

    channels.decRef(this.name);
    maybeMarkInactive(this);

    return true;
  }

  bindStore(store, transform) {
    const replacing = this._stores.has(store);
    if (!replacing) channels.incRef(this.name);
    this._stores.set(store, transform);
  }

  unbindStore(store) {
    if (!this._stores.has(store)) {
      return false;
    }

    this._stores.delete(store);

    channels.decRef(this.name);
    maybeMarkInactive(this);

    return true;
  }

  get hasSubscribers() {
    return true;
  }

  publish(data) {
    for (let i = 0; i < (this._subscribers?.length || 0); i++) {
      try {
        const onMessage = this._subscribers[i];
        onMessage(data, this.name);
      } catch (err) {
        nextTick(() => {
          // TODO(bartlomieju): in Node.js this is using `triggerUncaughtException` API, need
          // to clarify if we need that or if just throwing the error is enough here.
          throw err;
          // triggerUncaughtException(err, false);
        });
      }
    }
  }

  runStores(data, fn, thisArg, ...args) {
    let run = () => {
      this.publish(data);
      return ReflectApply(fn, thisArg, args);
    };

    for (const entry of this._stores.entries()) {
      const store = entry[0];
      const transform = entry[1];
      run = wrapStoreRun(store, data, run, transform);
    }

    return run();
  }
```

<a id="ref-q1-29"></a>
### [29] `tests/node_compat/config.jsonc:935-963`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L935-L963)

```
    "parallel/test-diagnostic-channel-http-request-created.js": {},
    "parallel/test-diagnostic-channel-http-response-created.js": {},
    "parallel/test-diagnostics-channel-bind-store.js": {},
    "parallel/test-diagnostics-channel-has-subscribers.js": {},
    "parallel/test-diagnostics-channel-http-server-start.js": {},
    "parallel/test-diagnostics-channel-http.js": {},
    "parallel/test-diagnostics-channel-http2-client-stream-close-error.js": {},
    "parallel/test-diagnostics-channel-http2-client-stream-error.js": {},
    "parallel/test-diagnostics-channel-http2-server-stream-close-error.js": {},
    "parallel/test-diagnostics-channel-http2-server-stream-error.js": {},
    "parallel/test-diagnostics-channel-object-channel-pub-sub.js": {},
    "parallel/test-diagnostics-channel-pub-sub.js": {},
    "parallel/test-diagnostics-channel-safe-subscriber-errors.js": {},
    "parallel/test-diagnostics-channel-symbol-named.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-callback-early-exit.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-callback-error.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-callback-run-stores.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-callback.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-has-subscribers.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-promise-early-exit.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-promise-error.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-promise-run-stores.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-promise-unhandled.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-promise.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-sync-early-exit.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-sync-error.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-sync-run-stores.js": {},
    "parallel/test-diagnostics-channel-tracing-channel-sync.js": {},
    "parallel/test-diagnostics-channel-udp.js": {},
```
