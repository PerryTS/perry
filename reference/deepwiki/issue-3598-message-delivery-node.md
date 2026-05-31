# DeepWiki Response

Repository: `nodejs/node`
Question: For Node.js worker_threads MessageChannel, MessagePort, and BroadcastChannel, summarize observable local delivery semantics for postMessage, message events/event handler properties, close(), ordering, receiveMessageOnPort, and same-name BroadcastChannel delivery. Include source-file citations where possible.
Search: https://deepwiki.com/search/for-nodejs-workerthreads-messa_69833a05-a653-4090-b373-359cf1dd8b85

**Question**: For Node.js worker_threads MessageChannel, MessagePort, and BroadcastChannel, summarize observable local delivery semantics for postMessage, message events/event handler properties, close(), ordering, receiveMessageOnPort, and same-name BroadcastChannel delivery. Include source-file citations where possible.

You're asking about the local delivery semantics for `postMessage`, `message` events, `close()`, message ordering, `receiveMessageOnPort`, and same-name `BroadcastChannel` delivery within Node.js `worker_threads` for `MessageChannel`, `MessagePort`, and `BroadcastChannel`.

### `MessageChannel` and `MessagePort`

`MessageChannel` creates a pair of `MessagePort` objects, `port1` and `port2`, which are linked for two-way communication [1](#ref-q1-1) .

#### `postMessage` and `message` events

When `port.postMessage(value[, transferList])` is called on one `MessagePort`, the `value` is sent to its linked `MessagePort` [2](#ref-q1-2) . The `value` is transferred using a structured clone algorithm, allowing for complex JavaScript values, including circular references, built-in types, and typed arrays [3](#ref-q1-3) . If `ArrayBuffer` instances are included in the `transferList`, their underlying memory is moved, making them unusable on the sending side [4](#ref-q1-4) . If not in `transferList`, the `ArrayBuffer` is copied [5](#ref-q1-5) . `SharedArrayBuffer` instances are accessible from both threads without being transferred [6](#ref-q1-6) .

The receiving `MessagePort` emits a `'message'` event, and its `onmessage` listener is invoked with the cloned `value` [7](#ref-q1-7) . Messages are processed in the order they are received [8](#ref-q1-8) . If deserialization fails, a `'messageerror'` event is emitted [9](#ref-q1-9) .

#### `close()`

Calling `port.close()` disables further sending of messages on both sides of the connection [10](#ref-q1-10) . A `'close'` event is emitted on both `MessagePort` instances in the channel [11](#ref-q1-11) .

#### `receiveMessageOnPort`

The `receiveMessageOnPort(port)` function allows synchronously receiving a single message from a given `MessagePort` [12](#ref-q1-12) . If no message is available, `undefined` is returned [13](#ref-q1-13) . When this function is used, no `'message'` event is emitted, and the `onmessage` listener is not invoked [14](#ref-q1-14) .

### `BroadcastChannel`

`BroadcastChannel` instances enable asynchronous one-to-many communication among all `BroadcastChannel` instances bound to the same channel name [15](#ref-q1-15) .

#### `postMessage` and `message` events

`broadcastChannel.postMessage(message)` sends a message to all other `BroadcastChannel` instances with the same name [16](#ref-q1-16) . The `message` is a cloneable JavaScript value [17](#ref-q1-17) . Each receiving `BroadcastChannel` instance emits a `'message'` event, and its `onmessage` property is invoked with a `MessageEvent` argument containing the `data` [18](#ref-q1-18)  [19](#ref-q1-19) . If a received message cannot be deserialized, an `onmessageerror` event is invoked [20](#ref-q1-20) .

#### `close()`

`broadcastChannel.close()` closes the `BroadcastChannel` connection, detaching its internal handle and preventing further messages from being sent or received through this instance [21](#ref-q1-21)  [22](#ref-q1-22) .

#### Same-name `BroadcastChannel` delivery

All `BroadcastChannel` instances created with the same `name` are connected [23](#ref-q1-23) . Messages posted to one instance are delivered to all other active instances sharing that name [15](#ref-q1-15) .

### Ordering

For both `MessagePort` and `BroadcastChannel`, messages are delivered in the order they are sent. The `MessagePort::OnMessage` function processes messages from an `incoming_messages_` queue, ensuring order [8](#ref-q1-8) .

## Notes

The `receiveMessageOnPort` function was updated in Node.js v15.12.0 to also accept a `BroadcastChannel` as its `port` argument [24](#ref-q1-24) . This means you can use `receiveMessageOnPort` to synchronously read messages from a `BroadcastChannel` as well, with the same implications regarding `onmessage` listeners not being invoked.

Wiki pages you might want to explore:
- [Core Runtime Architecture (nodejs/node)](/wiki/nodejs/node#2)

View this search on DeepWiki: https://deepwiki.com/search/for-nodejs-workerthreads-messa_69833a05-a653-4090-b373-359cf1dd8b85

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/worker_threads.md:1049-1054`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1049-L1054)

```markdown
Instances of the `worker.MessageChannel` class represent an asynchronous,
two-way communications channel.
The `MessageChannel` has no methods of its own. `new MessageChannel()`
yields an object with `port1` and `port2` properties, which refer to linked
[`MessagePort`][] instances.
```

<a id="ref-q1-2"></a>
### [2] `doc/api/worker_threads.md:1220-1221`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1220-L1221)

```markdown
Sends a JavaScript value to the receiving side of this channel.
`value` is transferred in a way which is compatible with
```

<a id="ref-q1-3"></a>
### [3] `doc/api/worker_threads.md:1222-1230`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1222-L1230)

```markdown
the [HTML structured clone algorithm][].

In particular, the significant differences to `JSON` are:

* `value` may contain circular references.
* `value` may contain instances of builtin JS types such as `RegExp`s,
  `BigInt`s, `Map`s, `Set`s, etc.
* `value` may contain typed arrays, both using `ArrayBuffer`s
  and `SharedArrayBuffer`s.
```

<a id="ref-q1-4"></a>
### [4] `doc/api/worker_threads.md:1268-1273`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1268-L1273)

```markdown
`transferList` may be a list of {ArrayBuffer}, [`MessagePort`][], and
[`FileHandle`][] objects.
After transferring, they are not usable on the sending side of the channel
anymore (even if they are not contained in `value`). Unlike with
[child processes][], transferring handles such as network sockets is currently
not supported.
```

<a id="ref-q1-5"></a>
### [5] `doc/api/worker_threads.md:1278-1280`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1278-L1280)

```markdown
`value` may still contain `ArrayBuffer` instances that are not in
`transferList`; in that case, the underlying memory is copied rather than moved.
```

<a id="ref-q1-6"></a>
### [6] `doc/api/worker_threads.md:1275-1276`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1275-L1276)

```markdown
If `value` contains {SharedArrayBuffer} instances, those are accessible
from either thread. They cannot be listed in `transferList`.
```

<a id="ref-q1-7"></a>
### [7] `doc/api/worker_threads.md:1141-1144`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1141-L1144)

```markdown
* `value` {any} The transmitted value

The `'message'` event is emitted for any incoming message, containing the cloned
input of [`port.postMessage()`][].
```

<a id="ref-q1-8"></a>
### [8] `src/node_messaging.cc:824-829`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_messaging.cc#L824-L829)

```cpp
    Mutex::ScopedLock lock(data_->mutex_);
    processing_limit = std::max(data_->incoming_messages_.size(),
                                static_cast<size_t>(1000));
  } else {
    processing_limit = std::numeric_limits<size_t>::max();
  }
```

<a id="ref-q1-9"></a>
### [9] `doc/api/worker_threads.md:1159-1160`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1159-L1160)

```markdown
The `'messageerror'` event is emitted when deserializing a message failed.
```

<a id="ref-q1-10"></a>
### [10] `doc/api/worker_threads.md:1173-1175`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1173-L1175)

```markdown
Disables further sending of messages on either side of the connection.
This method can be called when no further communication will happen over this
`MessagePort`.
```

<a id="ref-q1-11"></a>
### [11] `doc/api/worker_threads.md:1177-1178`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1177-L1178)

```markdown
The [`'close'` event][] is emitted on both `MessagePort` instances that
are part of the channel.
```

<a id="ref-q1-12"></a>
### [12] `doc/api/worker_threads.md:605-606`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L605-L606)

```markdown
Receive a single message from a given `MessagePort`. If no message is available,
`undefined` is returned, otherwise an object with a single `message` property
```

<a id="ref-q1-13"></a>
### [13] `doc/api/worker_threads.md:606-607`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L606-L607)

```markdown
`undefined` is returned, otherwise an object with a single `message` property
that contains the message payload, corresponding to the oldest message in the
```

<a id="ref-q1-14"></a>
### [14] `doc/api/worker_threads.md:634-636`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L634-L636)

```markdown
When this function is used, no `'message'` event is emitted and the
`onmessage` listener is not invoked.
```

<a id="ref-q1-15"></a>
### [15] `doc/api/worker_threads.md:928-930`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L928-L930)

```markdown
Instances of `BroadcastChannel` allow asynchronous one-to-many communication
with all other `BroadcastChannel` instances bound to the same channel name.
```

<a id="ref-q1-16"></a>
### [16] `doc/api/worker_threads.md:1015-1020`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1015-L1020)

```markdown

<!-- YAML
added: v15.4.0
-->

* `message` {any} Any cloneable JavaScript value.
```

<a id="ref-q1-17"></a>
### [17] `doc/api/worker_threads.md:1020`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1020)

```markdown
* `message` {any} Any cloneable JavaScript value.
```

<a id="ref-q1-18"></a>
### [18] `doc/api/worker_threads.md:1002-1004`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1002-L1004)

```markdown
* Type: {Function} Invoked with a single `MessageEvent` argument
  when a message is received.
```

<a id="ref-q1-19"></a>
### [19] `lib/internal/worker/io.js:348-350`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/worker/io.js#L348-L350)

```javascript
function onMessageEvent(type, data) {
  this.dispatchEvent(lazyMessageEvent(type, { data }));
}
```

<a id="ref-q1-20"></a>
### [20] `doc/api/worker_threads.md:1009-1012`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1009-L1012)

```markdown
-->

* Type: {Function} Invoked with a received message cannot be
  deserialized.
```

<a id="ref-q1-21"></a>
### [21] `doc/api/worker_threads.md:993-994`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L993-L994)

```markdown

Closes the `BroadcastChannel` connection.
```

<a id="ref-q1-22"></a>
### [22] `lib/internal/worker/io.js:403-414`
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

<a id="ref-q1-23"></a>
### [23] `doc/api/worker_threads.md:985-987`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L985-L987)

```markdown
* `name` {any} The name of the channel to connect to. Any JavaScript value
  that can be converted to a string using `` `${name}` `` is permitted.
```

<a id="ref-q1-24"></a>
### [24] `doc/api/worker_threads.md:597-598`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L597-L598)

```markdown
    pr-url: https://github.com/nodejs/node/pull/37535
    description: The port argument can also refer to a `BroadcastChannel` now.
```
