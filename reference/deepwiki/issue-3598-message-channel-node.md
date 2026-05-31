# DeepWiki Response

Repository: `nodejs/node`
Question: Explain Node.js implementation and public API behavior for globalThis MessageChannel, MessagePort, and BroadcastChannel and their worker_threads exports. Cover constructor identity, prototype/event behavior, close/postMessage semantics, transfer/listeners, and likely Node test/docs files.
Search: https://deepwiki.com/search/explain-nodejs-implementation_2485efd3-4d04-4404-a36b-3b77b49cc371

**Question**: Explain Node.js implementation and public API behavior for globalThis MessageChannel, MessagePort, and BroadcastChannel and their worker_threads exports. Cover constructor identity, prototype/event behavior, close/postMessage semantics, transfer/listeners, and likely Node test/docs files.

Node.js implements `MessageChannel`, `MessagePort`, and `BroadcastChannel` as part of its `worker_threads` module, providing mechanisms for inter-thread communication. These APIs are exposed globally in worker environments and are designed to be compatible with their browser counterparts where applicable [1](#ref-q1-1) .

## `MessageChannel`

The `MessageChannel` class creates an asynchronous, two-way communication channel [2](#ref-q1-2) . A new `MessageChannel()` instance yields an object with two linked `MessagePort` instances, `port1` and `port2` [3](#ref-q1-3) .

### Constructor Identity and Usage
The `MessageChannel` constructor is exposed through `require('node:worker_threads').MessageChannel` [4](#ref-q1-4) . In C++, the `MessageChannel` constructor is set up in `CreatePerIsolateProperties` within `src/node_messaging.cc` [5](#ref-q1-5) .

## `MessagePort`

`MessagePort` instances represent one end of a communication channel, used for transferring structured data, memory regions, and other `MessagePort`s between `Worker`s [6](#ref-q1-6) . Node.js's `MessagePort` implementation aims to match browser `MessagePort`s [7](#ref-q1-7) .

### Constructor Identity
`MessagePort` is exported by the `worker_threads` module [8](#ref-q1-8) . The C++ implementation defines the `MessagePort` class and its constructor [9](#ref-q1-9)  [10](#ref-q1-10) .

### Prototype and Event Behavior
`MessagePort` extends `EventTarget` [11](#ref-q1-11) , meaning it supports event listeners like `on('message')` and `on('close')`.
*   **`'message'` event**: Emitted for any incoming message, containing a clone of the value passed to `postMessage()` [12](#ref-q1-12) .
*   **`'messageerror'` event**: Emitted when deserializing a message fails, for example, when Node.js API objects are received in a `vm.Context` where they are unavailable [13](#ref-q1-13) .
*   **`'close'` event**: Emitted once either side of the channel has been disconnected [14](#ref-q1-14) .

### `close()` and `postMessage()` Semantics
*   **`port.close()`**: Disables further sending of messages on either side of the connection and emits a `'close'` event on both `MessagePort` instances [15](#ref-q1-15) . The C++ implementation handles the `Close` operation [16](#ref-q1-16) .
*   **`port.postMessage(value[, transferList])`**: Sends a JavaScript value to the receiving side. The `value` is transferred using a mechanism compatible with the HTML structured clone algorithm, supporting circular references, built-in JS types, typed arrays, and `WebAssembly.Module` instances [17](#ref-q1-17) . Native (C++-backed) objects generally cannot be transferred, with specific exceptions like `CryptoKey`s and other listed types [18](#ref-q1-18) .

### Transfer and Listeners
The `transferList` argument in `postMessage` allows transferring `ArrayBuffer`, `MessagePort`, and `FileHandle` objects [19](#ref-q1-19) . After transfer, these objects are no longer usable on the sending side [20](#ref-q1-20) . `SharedArrayBuffer` instances are accessible from both threads and cannot be listed in `transferList` [21](#ref-q1-21) . If an `ArrayBuffer` is not in the `transferList`, its memory is copied [22](#ref-q1-22) .

The `receiveMessageOnPort(port)` function allows synchronously receiving a single message from a `MessagePort` or `BroadcastChannel`. When used, no `'message'` event is emitted, and the `onmessage` listener is not invoked [23](#ref-q1-23) .

## `BroadcastChannel`

The `BroadcastChannel` class enables asynchronous one-to-many communication with all other `BroadcastChannel` instances bound to the same channel name [24](#ref-q1-24) .

### Constructor Identity
`BroadcastChannel` is exported by the `worker_threads` module [25](#ref-q1-25) . Its constructor takes a `name` argument, which can be any JavaScript value convertible to a string [26](#ref-q1-26) . The internal implementation in `lib/internal/worker/io.js` shows it extending `EventTarget` and using an internal `broadcastChannel` binding [27](#ref-q1-27) .

### Prototype and Event Behavior
`BroadcastChannel` extends `EventTarget` [28](#ref-q1-28) .
*   **`onmessage`**: Invoked with a `MessageEvent` argument when a message is received [29](#ref-q1-29) .
*   **`onmessageerror`**: Invoked when a received message cannot be deserialized [30](#ref-q1-30) .

### `close()` and `postMessage()` Semantics
*   **`broadcastChannel.close()`**: Closes the `BroadcastChannel` connection [31](#ref-q1-31) . The internal implementation removes event listeners and closes the underlying handle [32](#ref-q1-32) .
*   **`broadcastChannel.postMessage(message)`**: Sends any cloneable JavaScript value to other instances of the channel [33](#ref-q1-33) .

## `worker_threads` Exports
The `worker_threads` module exports `MessagePort`, `MessageChannel`, and `BroadcastChannel` directly [34](#ref-q1-34)  [25](#ref-q1-25) . It also provides `moveMessagePortToContext` for transferring a `MessagePort` to a different `vm` Context [35](#ref-q1-35) .

## Node.js Test and Documentation Files
*   **Documentation**: The primary documentation for these APIs is found in `doc/api/worker_threads.md` [36](#ref-q1-36) .
*   **Tests**: `test/parallel/test-worker-workerdata-messageport.js` contains tests demonstrating the behavior of `MessageChannel` and `MessagePort`, including transfer list semantics and error handling [37](#ref-q1-37) .

## Notes
The `globalThis` exposure of `MessageChannel`, `MessagePort`, and `BroadcastChannel` is handled by `lib/internal/bootstrap/web/exposed-window-or-worker.js`, which uses `exposeLazyInterfaces` to make these available [1](#ref-q1-1) . The C++ bindings for these messaging primitives are defined in `src/node_messaging.h` and `src/node_messaging.cc` [38](#ref-q1-38)  [39](#ref-q1-39) .

Wiki pages you might want to explore:
- [Core Runtime Architecture (nodejs/node)](/wiki/nodejs/node#2)

View this search on DeepWiki: https://deepwiki.com/search/explain-nodejs-implementation_2485efd3-4d04-4404-a36b-3b77b49cc371

## References

<a id="ref-q1-1"></a>
### [1] `lib/internal/bootstrap/web/exposed-window-or-worker.js:49-52`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/bootstrap/web/exposed-window-or-worker.js#L49-L52)

```javascript
// https://html.spec.whatwg.org/multipage/web-messaging.html#broadcasting-to-other-browsing-contexts
exposeLazyInterfaces(globalThis, 'internal/worker/io', ['BroadcastChannel']);
exposeLazyInterfaces(globalThis, 'internal/worker/io', [
  'MessageChannel', 'MessagePort',
```

<a id="ref-q1-2"></a>
### [2] `doc/api/worker_threads.md:1049-1050`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1049-L1050)

```markdown
Instances of the `worker.MessageChannel` class represent an asynchronous,
two-way communications channel.
```

<a id="ref-q1-3"></a>
### [3] `doc/api/worker_threads.md:1051-1053`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1051-L1053)

```markdown
The `MessageChannel` has no methods of its own. `new MessageChannel()`
yields an object with `port1` and `port2` properties, which refer to linked
[`MessagePort`][] instances.
```

<a id="ref-q1-4"></a>
### [4] `lib/worker_threads.js:39`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/worker_threads.js#L39)

```javascript
  MessageChannel,
```

<a id="ref-q1-5"></a>
### [5] `src/node_messaging.cc:1713-1716`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_messaging.cc#L1713-L1716)

```cpp
    SetConstructorFunction(isolate,
                           target,
                           "MessageChannel",
                           NewFunctionTemplate(isolate, MessageChannel));
```

<a id="ref-q1-6"></a>
### [6] `doc/api/worker_threads.md:1089-1093`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1089-L1093)

```markdown
Instances of the `worker.MessagePort` class represent one end of an
asynchronous, two-way communications channel. It can be used to transfer
structured data, memory regions and other `MessagePort`s between different
[`Worker`][]s.
```

<a id="ref-q1-7"></a>
### [7] `doc/api/worker_threads.md:1095`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1095)

<a id="ref-q1-8"></a>
### [8] `lib/worker_threads.js:38`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/worker_threads.js#L38)

```javascript
  MessagePort,
```

<a id="ref-q1-9"></a>
### [9] `src/node_messaging.h:225-233`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_messaging.h#L225-L233)

```c
class MessagePort : public HandleWrap {
 private:
  // Create a new MessagePort. The `context` argument specifies the Context
  // instance that is used for creating the values emitted from this port.
  // This is called by MessagePort::New(), which is the public API used for
  // creating MessagePort instances.
  MessagePort(Environment* env,
              v8::Local<v8::Context> context,
              v8::Local<v8::Object> wrap);
```

<a id="ref-q1-10"></a>
### [10] `src/node_messaging.h:259`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_messaging.h#L259)

```c
  static void New(const v8::FunctionCallbackInfo<v8::Value>& args);
```

<a id="ref-q1-11"></a>
### [11] `doc/api/worker_threads.md:1087`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1087)

```markdown
* Extends: {EventTarget}
```

<a id="ref-q1-12"></a>
### [12] `doc/api/worker_threads.md:1143-1145`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1143-L1145)

```markdown
The `'message'` event is emitted for any incoming message, containing the cloned
input of [`port.postMessage()`][].
```

<a id="ref-q1-13"></a>
### [13] `doc/api/worker_threads.md:1157-1165`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1157-L1165)

```markdown
* `error` {Error} An Error object

The `'messageerror'` event is emitted when deserializing a message failed.

Currently, this event is emitted when there is an error occurring while
instantiating the posted JS object on the receiving end. Such situations
are rare, but can happen, for instance, when certain Node.js API objects
are received in a `vm.Context` (where Node.js APIs are currently
unavailable).
```

<a id="ref-q1-14"></a>
### [14] `doc/api/worker_threads.md:1102-1103`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1102-L1103)

```markdown
The `'close'` event is emitted once either side of the channel has been
disconnected.
```

<a id="ref-q1-15"></a>
### [15] `doc/api/worker_threads.md:1173-1178`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1173-L1178)

```markdown
Disables further sending of messages on either side of the connection.
This method can be called when no further communication will happen over this
`MessagePort`.

The [`'close'` event][] is emitted on both `MessagePort` instances that
are part of the channel.
```

<a id="ref-q1-16"></a>
### [16] `src/node_messaging.h:280-281`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_messaging.h#L280-L281)

```c
  void Close(
      v8::Local<v8::Value> close_callback = v8::Local<v8::Value>()) override;
```

<a id="ref-q1-17"></a>
### [17] `doc/api/worker_threads.md:1220-1231`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1220-L1231)

```markdown
Sends a JavaScript value to the receiving side of this channel.
`value` is transferred in a way which is compatible with
the [HTML structured clone algorithm][].

In particular, the significant differences to `JSON` are:

* `value` may contain circular references.
* `value` may contain instances of builtin JS types such as `RegExp`s,
  `BigInt`s, `Map`s, `Set`s, etc.
* `value` may contain typed arrays, both using `ArrayBuffer`s
  and `SharedArrayBuffer`s.
* `value` may contain [`WebAssembly.Module`][] instances.
```

<a id="ref-q1-18"></a>
### [18] `doc/api/worker_threads.md:1232-1240`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1232-L1240)

```markdown
* `value` may not contain native (C++-backed) objects other than:
  * {CryptoKey}s,
  * {FileHandle}s,
  * {Histogram}s,
  * {KeyObject}s,
  * {MessagePort}s,
  * {net.BlockList}s,
  * {net.SocketAddress}es,
  * {X509Certificate}s.
```

<a id="ref-q1-19"></a>
### [19] `doc/api/worker_threads.md:1268-1270`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1268-L1270)

```markdown
`transferList` may be a list of {ArrayBuffer}, [`MessagePort`][], and
[`FileHandle`][] objects.
After transferring, they are not usable on the sending side of the channel
```

<a id="ref-q1-20"></a>
### [20] `doc/api/worker_threads.md:1271-1273`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1271-L1273)

```markdown
anymore (even if they are not contained in `value`). Unlike with
[child processes][], transferring handles such as network sockets is currently
not supported.
```

<a id="ref-q1-21"></a>
### [21] `doc/api/worker_threads.md:1275-1276`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1275-L1276)

```markdown
If `value` contains {SharedArrayBuffer} instances, those are accessible
from either thread. They cannot be listed in `transferList`.
```

<a id="ref-q1-22"></a>
### [22] `doc/api/worker_threads.md:1278-1279`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1278-L1279)

```markdown
`value` may still contain `ArrayBuffer` instances that are not in
`transferList`; in that case, the underlying memory is copied rather than moved.
```

<a id="ref-q1-23"></a>
### [23] `doc/api/worker_threads.md:634-635`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L634-L635)

```markdown
When this function is used, no `'message'` event is emitted and the
`onmessage` listener is not invoked.
```

<a id="ref-q1-24"></a>
### [24] `doc/api/worker_threads.md:928-930`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L928-L930)

```markdown
Instances of `BroadcastChannel` allow asynchronous one-to-many communication
with all other `BroadcastChannel` instances bound to the same channel name.
```

<a id="ref-q1-25"></a>
### [25] `lib/worker_threads.js:53`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/worker_threads.js#L53)

```javascript
  BroadcastChannel,
```

<a id="ref-q1-26"></a>
### [26] `doc/api/worker_threads.md:984-987`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L984-L987)

```markdown

* `name` {any} The name of the channel to connect to. Any JavaScript value
  that can be converted to a string using `` `${name}` `` is permitted.
```

<a id="ref-q1-27"></a>
### [27] `lib/internal/worker/io.js:356-366`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/worker/io.js#L356-L366)

```javascript
class BroadcastChannel extends EventTarget {
  /**
   * @param {string} name
   */
  constructor(name) {
    if (arguments.length === 0)
      throw new ERR_MISSING_ARGS('name');
    super();
    this[kType] = 'BroadcastChannel';
    this[kName] = `${name}`;
    this[kHandle] = broadcastChannel(this[kName]);
```

<a id="ref-q1-28"></a>
### [28] `doc/api/worker_threads.md:918`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L918)

```markdown
## Class: `BroadcastChannel extends EventTarget`
```

<a id="ref-q1-29"></a>
### [29] `doc/api/worker_threads.md:996-1003`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L996-L1003)

```markdown
### `broadcastChannel.onmessage`

<!-- YAML
added: v15.4.0
-->

* Type: {Function} Invoked with a single `MessageEvent` argument
  when a message is received.
```

<a id="ref-q1-30"></a>
### [30] `doc/api/worker_threads.md:1005-1013`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1005-L1013)

```markdown
### `broadcastChannel.onmessageerror`

<!-- YAML
added: v15.4.0
-->

* Type: {Function} Invoked with a received message cannot be
  deserialized.
```

<a id="ref-q1-31"></a>
### [31] `doc/api/worker_threads.md:989-994`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L989-L994)

```markdown

<!-- YAML
added: v15.4.0
-->

Closes the `BroadcastChannel` connection.
```

<a id="ref-q1-32"></a>
### [32] `lib/internal/worker/io.js:403-414`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/worker/io.js#L403-L414)

```javascript
  close() {
    if (!isBroadcastChannel(this))
      throw new ERR_INVALID_THIS('BroadcastChannel');
    if (this[kHandle] === undefined)
      return;
    this[kHandle].off('message', this[kOnMessage]);
    this[kHandle].off('messageerror', this[kOnMessageError]);
    this[kOnMessage] = undefined;
    this[kOnMessageError] = undefined;
    this[kHandle].close();
    this[kHandle] = undefined;
  }
```

<a id="ref-q1-33"></a>
### [33] `doc/api/worker_threads.md:1014-1020`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1014-L1020)

```markdown
### `broadcastChannel.postMessage(message)`

<!-- YAML
added: v15.4.0
-->

* `message` {any} Any cloneable JavaScript value.
```

<a id="ref-q1-34"></a>
### [34] `lib/worker_threads.js:38-40`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/worker_threads.js#L38-L40)

```javascript
  MessagePort,
  MessageChannel,
  markAsUncloneable,
```

<a id="ref-q1-35"></a>
### [35] `doc/api/worker_threads.md:402-417`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L402-L417)

```markdown
## `worker_threads.moveMessagePortToContext(port, contextifiedSandbox)`

<!-- YAML
added: v11.13.0
-->

* `port` {MessagePort} The message port to transfer.

* `contextifiedSandbox` {Object} A [contextified][] object as returned by the
  `vm.createContext()` method.

* Returns: {MessagePort}

Transfer a `MessagePort` to a different [`vm`][] Context. The original `port`
object is rendered unusable, and the returned `MessagePort` instance
takes its place.
```

<a id="ref-q1-36"></a>
### [36] `doc/api/worker_threads.md:918-1347`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L918-L1347)

```markdown
## Class: `BroadcastChannel extends EventTarget`

<!-- YAML
added: v15.4.0
changes:
  - version: v18.0.0
    pr-url: https://github.com/nodejs/node/pull/41271
    description: No longer experimental.
-->

Instances of `BroadcastChannel` allow asynchronous one-to-many communication
with all other `BroadcastChannel` instances bound to the same channel name.

```mjs
import {
  isMainThread,
  BroadcastChannel,
  Worker,
} from 'node:worker_threads';

const bc = new BroadcastChannel('hello');

if (isMainThread) {
  let c = 0;
  bc.onmessage = (event) => {
    console.log(event.data);
    if (++c === 10) bc.close();
  };
  for (let n = 0; n < 10; n++)
    new Worker(new URL(import.meta.url));
} else {
  bc.postMessage('hello from every worker');
  bc.close();
}
```

```cjs
'use strict';

const {
  isMainThread,
  BroadcastChannel,
  Worker,
} = require('node:worker_threads');

const bc = new BroadcastChannel('hello');

if (isMainThread) {
  let c = 0;
  bc.onmessage = (event) => {
    console.log(event.data);
    if (++c === 10) bc.close();
  };
  for (let n = 0; n < 10; n++)
    new Worker(__filename);
} else {
  bc.postMessage('hello from every worker');
  bc.close();
}
```

### `new BroadcastChannel(name)`

<!-- YAML
added: v15.4.0
-->

* `name` {any} The name of the channel to connect to. Any JavaScript value
  that can be converted to a string using `` `${name}` `` is permitted.

### `broadcastChannel.close()`

<!-- YAML
added: v15.4.0
-->

Closes the `BroadcastChannel` connection.

### `broadcastChannel.onmessage`

<!-- YAML
added: v15.4.0
-->

* Type: {Function} Invoked with a single `MessageEvent` argument
  when a message is received.

### `broadcastChannel.onmessageerror`

<!-- YAML
added: v15.4.0
-->

* Type: {Function} Invoked with a received message cannot be
  deserialized.

### `broadcastChannel.postMessage(message)`

<!-- YAML
added: v15.4.0
-->

* `message` {any} Any cloneable JavaScript value.

### `broadcastChannel.ref()`

<!-- YAML
added: v15.4.0
-->

Opposite of `unref()`. Calling `ref()` on a previously `unref()`ed
BroadcastChannel does _not_ let the program exit if it's the only active handle
left (the default behavior). If the port is `ref()`ed, calling `ref()` again
has no effect.

### `broadcastChannel.unref()`

<!-- YAML
added: v15.4.0
-->

Calling `unref()` on a BroadcastChannel allows the thread to exit if this
is the only active handle in the event system. If the BroadcastChannel is
already `unref()`ed calling `unref()` again has no effect.

## Class: `MessageChannel`

<!-- YAML
added: v10.5.0
-->

Instances of the `worker.MessageChannel` class represent an asynchronous,
two-way communications channel.
The `MessageChannel` has no methods of its own. `new MessageChannel()`
yields an object with `port1` and `port2` properties, which refer to linked
[`MessagePort`][] instances.

```mjs
import { MessageChannel } from 'node:worker_threads';

const { port1, port2 } = new MessageChannel();
port1.on('message', (message) => console.log('received', message));
port2.postMessage({ foo: 'bar' });
// Prints: received { foo: 'bar' } from the `port1.on('message')` listener
```

```cjs
'use strict';

const { MessageChannel } = require('node:worker_threads');

const { port1, port2 } = new MessageChannel();
port1.on('message', (message) => console.log('received', message));
port2.postMessage({ foo: 'bar' });
// Prints: received { foo: 'bar' } from the `port1.on('message')` listener
```

## Class: `MessagePort`

<!-- YAML
added: v10.5.0
changes:
  - version:
    - v14.7.0
    pr-url: https://github.com/nodejs/node/pull/34057
    description: This class now inherits from `EventTarget` rather than
                 from `EventEmitter`.
-->

* Extends: {EventTarget}

Instances of the `worker.MessagePort` class represent one end of an
asynchronous, two-way communications channel. It can be used to transfer
structured data, memory regions and other `MessagePort`s between different
[`Worker`][]s.

This implementation matches [browser `MessagePort`][]s.

### Event: `'close'`

<!-- YAML
added: v10.5.0
-->

The `'close'` event is emitted once either side of the channel has been
disconnected.

```mjs
import { MessageChannel } from 'node:worker_threads';
const { port1, port2 } = new MessageChannel();

// Prints:
//   foobar
//   closed!
port2.on('message', (message) => console.log(message));
port2.once('close', () => console.log('closed!'));

port1.postMessage('foobar');
port1.close();
```

```cjs
'use strict';

const { MessageChannel } = require('node:worker_threads');
const { port1, port2 } = new MessageChannel();

// Prints:
//   foobar
//   closed!
port2.on('message', (message) => console.log(message));
port2.once('close', () => console.log('closed!'));

port1.postMessage('foobar');
port1.close();
```

### Event: `'message'`

<!-- YAML
added: v10.5.0
-->

* `value` {any} The transmitted value

The `'message'` event is emitted for any incoming message, containing the cloned
input of [`port.postMessage()`][].

Listeners on this event receive a clone of the `value` parameter as passed
to `postMessage()` and no further arguments.

### Event: `'messageerror'`

<!-- YAML
added:
  - v14.5.0
  - v12.19.0
-->

* `error` {Error} An Error object

The `'messageerror'` event is emitted when deserializing a message failed.

Currently, this event is emitted when there is an error occurring while
instantiating the posted JS object on the receiving end. Such situations
are rare, but can happen, for instance, when certain Node.js API objects
are received in a `vm.Context` (where Node.js APIs are currently
unavailable).

### `port.close()`

<!-- YAML
added: v10.5.0
-->

Disables further sending of messages on either side of the connection.
This method can be called when no further communication will happen over this
`MessagePort`.

The [`'close'` event][] is emitted on both `MessagePort` instances that
are part of the channel.

### `port.postMessage(value[, transferList])`

<!-- YAML
added: v10.5.0
changes:
  - version: v21.0.0
    pr-url: https://github.com/nodejs/node/pull/47604
    description: An error is thrown when an untransferable object is in the
                 transfer list.
  - version:
      - v15.14.0
      - v14.18.0
    pr-url: https://github.com/nodejs/node/pull/37917
    description: Add 'BlockList' to the list of cloneable types.
  - version:
      - v15.9.0
      - v14.18.0
    pr-url: https://github.com/nodejs/node/pull/37155
    description: Add 'Histogram' types to the list of cloneable types.
  - version: v15.6.0
    pr-url: https://github.com/nodejs/node/pull/36804
    description: Added `X509Certificate` to the list of cloneable types.
  - version: v15.0.0
    pr-url: https://github.com/nodejs/node/pull/35093
    description: Added `CryptoKey` to the list of cloneable types.
  - version:
    - v14.5.0
    - v12.19.0
    pr-url: https://github.com/nodejs/node/pull/33360
    description: Added `KeyObject` to the list of cloneable types.
  - version:
    - v14.5.0
    - v12.19.0
    pr-url: https://github.com/nodejs/node/pull/33772
    description: Added `FileHandle` to the list of transferable types.
-->

* `value` {any}
* `transferList` {Object\[]}

Sends a JavaScript value to the receiving side of this channel.
`value` is transferred in a way which is compatible with
the [HTML structured clone algorithm][].

In particular, the significant differences to `JSON` are:

* `value` may contain circular references.
* `value` may contain instances of builtin JS types such as `RegExp`s,
  `BigInt`s, `Map`s, `Set`s, etc.
* `value` may contain typed arrays, both using `ArrayBuffer`s
  and `SharedArrayBuffer`s.
* `value` may contain [`WebAssembly.Module`][] instances.
* `value` may not contain native (C++-backed) objects other than:
  * {CryptoKey}s,
  * {FileHandle}s,
  * {Histogram}s,
  * {KeyObject}s,
  * {MessagePort}s,
  * {net.BlockList}s,
  * {net.SocketAddress}es,
  * {X509Certificate}s.

```mjs
import { MessageChannel } from 'node:worker_threads';
const { port1, port2 } = new MessageChannel();

port1.on('message', (message) => console.log(message));

const circularData = {};
circularData.foo = circularData;
// Prints: { foo: [Circular] }
port2.postMessage(circularData);
```

```cjs
'use strict';

const { MessageChannel } = require('node:worker_threads');
const { port1, port2 } = new MessageChannel();

port1.on('message', (message) => console.log(message));

const circularData = {};
circularData.foo = circularData;
// Prints: { foo: [Circular] }
port2.postMessage(circularData);
```

`transferList` may be a list of {ArrayBuffer}, [`MessagePort`][], and
[`FileHandle`][] objects.
After transferring, they are not usable on the sending side of the channel
anymore (even if they are not contained in `value`). Unlike with
[child processes][], transferring handles such as network sockets is currently
not supported.

If `value` contains {SharedArrayBuffer} instances, those are accessible
from either thread. They cannot be listed in `transferList`.

`value` may still contain `ArrayBuffer` instances that are not in
`transferList`; in that case, the underlying memory is copied rather than moved.

```mjs
import { MessageChannel } from 'node:worker_threads';
const { port1, port2 } = new MessageChannel();

port1.on('message', (message) => console.log(message));

const uint8Array = new Uint8Array([ 1, 2, 3, 4 ]);
// This posts a copy of `uint8Array`:
port2.postMessage(uint8Array);
// This does not copy data, but renders `uint8Array` unusable:
port2.postMessage(uint8Array, [ uint8Array.buffer ]);

// The memory for the `sharedUint8Array` is accessible from both the
// original and the copy received by `.on('message')`:
const sharedUint8Array = new Uint8Array(new SharedArrayBuffer(4));
port2.postMessage(sharedUint8Array);

// This transfers a freshly created message port to the receiver.
// This can be used, for example, to create communication channels between
// multiple `Worker` threads that are children of the same parent thread.
const otherChannel = new MessageChannel();
port2.postMessage({ port: otherChannel.port1 }, [ otherChannel.port1 ]);
```

```cjs
'use strict';

const { MessageChannel } = require('node:worker_threads');
const { port1, port2 } = new MessageChannel();

port1.on('message', (message) => console.log(message));

const uint8Array = new Uint8Array([ 1, 2, 3, 4 ]);
// This posts a copy of `uint8Array`:
port2.postMessage(uint8Array);
// This does not copy data, but renders `uint8Array` unusable:
port2.postMessage(uint8Array, [ uint8Array.buffer ]);

// The memory for the `sharedUint8Array` is accessible from both the
// original and the copy received by `.on('message')`:
const sharedUint8Array = new Uint8Array(new SharedArrayBuffer(4));
port2.postMessage(sharedUint8Array);

// This transfers a freshly created message port to the receiver.
// This can be used, for example, to create communication channels between
// multiple `Worker` threads that are children of the same parent thread.
const otherChannel = new MessageChannel();
port2.postMessage({ port: otherChannel.port1 }, [ otherChannel.port1 ]);
```

The message object is cloned immediately, and can be modified after
posting without having side effects.

For more information on the serialization and deserialization mechanisms
behind this API, see the [serialization API of the `node:v8` module][v8.serdes].

#### Considerations when transferring TypedArrays and Buffers

All {TypedArray|Buffer} instances are views over an underlying
{ArrayBuffer}. That is, it is the `ArrayBuffer` that actually stores
the raw data while the `TypedArray` and `Buffer` objects provide a
way of viewing and manipulating the data. It is possible and common
for multiple views to be created over the same `ArrayBuffer` instance.
Great care must be taken when using a transfer list to transfer an
`ArrayBuffer` as doing so causes all `TypedArray` and `Buffer`
instances that share that same `ArrayBuffer` to become unusable.
```

<a id="ref-q1-37"></a>
### [37] `test/parallel/test-worker-workerdata-messageport.js:1-91`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-workerdata-messageport.js#L1-L91)

<a id="ref-q1-38"></a>
### [38] `src/node_messaging.h:225-319`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_messaging.h#L225-L319)

```c
class MessagePort : public HandleWrap {
 private:
  // Create a new MessagePort. The `context` argument specifies the Context
  // instance that is used for creating the values emitted from this port.
  // This is called by MessagePort::New(), which is the public API used for
  // creating MessagePort instances.
  MessagePort(Environment* env,
              v8::Local<v8::Context> context,
              v8::Local<v8::Object> wrap);

 public:
  ~MessagePort() override;

  // Create a new message port instance, optionally over an existing
  // `MessagePortData` object.
  static MessagePort* New(Environment* env,
                          v8::Local<v8::Context> context,
                          std::unique_ptr<MessagePortData> data = {},
                          std::shared_ptr<SiblingGroup> sibling_group = {});

  // Send a message, i.e. deliver it into the sibling's incoming queue.
  // If this port is closed, or if there is no sibling, this message is
  // serialized with transfers, then silently discarded.
  v8::Maybe<bool> PostMessage(Environment* env,
                              v8::Local<v8::Context> context,
                              v8::Local<v8::Value> message,
                              const TransferList& transfer);

  // Start processing messages on this port as a receiving end.
  void Start();
  // Stop processing messages on this port as a receiving end.
  void Stop();

  /* constructor */
  static void New(const v8::FunctionCallbackInfo<v8::Value>& args);
  /* prototype methods */
  static void PostMessage(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void Start(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void Stop(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void Drain(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void ReceiveMessage(const v8::FunctionCallbackInfo<v8::Value>& args);

  /* static */
  static void MoveToContext(const v8::FunctionCallbackInfo<v8::Value>& args);

  // Turns `a` and `b` into siblings, i.e. connects the sending side of one
  // to the receiving side of the other. This is not thread-safe.
  static void Entangle(MessagePort* a, MessagePort* b);
  static void Entangle(MessagePort* a, MessagePortData* b);

  // Detach this port's data for transferring. After this, the MessagePortData
  // is no longer associated with this handle, although it can still receive
  // messages.
  std::unique_ptr<MessagePortData> Detach();

  void Close(
      v8::Local<v8::Value> close_callback = v8::Local<v8::Value>()) override;

  // Returns true if either data_ has been freed, or if the handle is being
  // closed. Equivalent to the [[Detached]] internal slot in the HTML Standard.
  //
  // If checking if a JavaScript MessagePort object is detached, this method
  // alone is often not enough, since the backing C++ MessagePort object may
  // have been deleted already. For all intents and purposes, an object with a
  // NULL pointer to the C++ MessagePort object is also detached.
  inline bool IsDetached() const;

  BaseObject::TransferMode GetTransferMode() const override;
  std::unique_ptr<TransferData> TransferForMessaging() override;

  void MemoryInfo(MemoryTracker* tracker) const override;
  SET_MEMORY_INFO_NAME(MessagePort)
  SET_SELF_SIZE(MessagePort)

 private:
  enum class MessageProcessingMode {
    kNormalOperation,
    kForceReadMessages
  };

  void OnClose() override;
  void OnMessage(MessageProcessingMode mode);
  void TriggerAsync();
  v8::MaybeLocal<v8::Value> ReceiveMessage(
      v8::Local<v8::Context> context,
      MessageProcessingMode mode,
      v8::Local<v8::Value>* port_list = nullptr);

  std::unique_ptr<MessagePortData> data_ = nullptr;
  bool receiving_messages_ = false;
  uv_async_t async_;
  v8::Global<v8::Function> emit_message_fn_;

  friend class MessagePortData;
};
```

<a id="ref-q1-39"></a>
### [39] `src/node_messaging.cc:1708-1750`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_messaging.cc#L1708-L1750)

```cpp
static void CreatePerIsolateProperties(IsolateData* isolate_data,
                                       Local<ObjectTemplate> target) {
  Isolate* isolate = isolate_data->isolate();

  {
    SetConstructorFunction(isolate,
                           target,
                           "MessageChannel",
                           NewFunctionTemplate(isolate, MessageChannel));
  }

  {
    Local<FunctionTemplate> t = FunctionTemplate::New(isolate);
    t->InstanceTemplate()->SetInternalFieldCount(
        JSTransferable::kInternalFieldCount);
    t->SetClassName(FIXED_ONE_BYTE_STRING(isolate, "JSTransferable"));
    isolate_data->set_js_transferable_constructor_template(t);
  }

  SetConstructorFunction(isolate,
                         target,
                         isolate_data->message_port_constructor_string(),
                         GetMessagePortConstructorTemplate(isolate_data));

  SetMethod(isolate,
            target,
            "exposeLazyDOMExceptionProperty",
            ExposeLazyDOMExceptionProperty);
  // These are not methods on the MessagePort prototype, because
  // the browser equivalents do not provide them.
  SetMethod(isolate, target, "stopMessagePort", MessagePort::Stop);
  SetMethod(isolate, target, "drainMessagePort", MessagePort::Drain);
  SetMethod(
      isolate, target, "receiveMessageOnPort", MessagePort::ReceiveMessage);
  SetMethod(
      isolate, target, "moveMessagePortToContext", MessagePort::MoveToContext);
  SetMethod(isolate,
            target,
            "setDeserializerCreateObjectFunction",
            SetDeserializerCreateObjectFunction);
  SetMethod(isolate, target, "broadcastChannel", BroadcastChannel);
  SetMethod(isolate, target, "structuredClone", StructuredClone);
}
```
