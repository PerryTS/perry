# DeepWiki Response

Repository: `nodejs/node`
Question: How does node:worker_threads postMessageToThread deliver process workerMessage events and reject same-thread or timeout cases?
Search: https://deepwiki.com/search/how-does-nodeworkerthreads-pos_f36a0adf-2459-4416-ad5e-d206656740ce

**Question**: How does node:worker_threads postMessageToThread deliver process workerMessage events and reject same-thread or timeout cases?

The user is asking about how `node:worker_threads.postMessageToThread()` delivers `process.workerMessage` events and handles same-thread or timeout scenarios.

`node:worker_threads.postMessageToThread()` is a function that sends a value to another worker thread identified by its thread ID [1](#ref-q1-1) . This function returns a Promise that is fulfilled if the message is successfully processed by the destination thread [2](#ref-q1-2) .

### Delivery of `process.workerMessage` events

When `postMessageToThread()` is called, the message is delivered to the target thread, which then emits a `'workerMessage'` event on the `process` object [3](#ref-q1-3) . The event listener for `'workerMessage'` receives the `value` sent and the `source` thread ID [4](#ref-q1-4) .

An example of this can be seen in the Node.js documentation, where a main thread sets up a listener for `'workerMessage'` and then uses `postMessageToThread` to communicate with other workers [5](#ref-q1-5) .

### Rejection Cases

`postMessageToThread()` handles several error conditions by throwing specific errors:

*   **Same-thread messaging**: If the `threadId` provided to `postMessageToThread()` is the same as the current thread's ID, an `ERR_WORKER_MESSAGING_SAME_THREAD` error is thrown [6](#ref-q1-6) . This error is defined as "Cannot sent a message to the same thread" [7](#ref-q1-7) .
*   **Timeout**: If a `timeout` is specified and the message delivery times out, an `ERR_WORKER_MESSAGING_TIMEOUT` error is thrown [8](#ref-q1-8) . This error is defined as "Sending a message to another thread timed out" [9](#ref-q1-9) .
*   **Invalid thread or no listener**: If the target `threadId` is invalid or the target thread does not have a listener for the `workerMessage` event, an `ERR_WORKER_MESSAGING_FAILED` error is thrown [10](#ref-q1-10) . This error is defined as "Cannot find the destination thread or listener" [11](#ref-q1-11) .
*   **Error during processing**: If the destination thread throws an error while processing the `workerMessage` event, an `ERR_WORKER_MESSAGING_ERRORED` error is thrown [12](#ref-q1-12) . This error is defined as "The destination thread threw an error while processing the message" [13](#ref-q1-13) .

## Notes

The `postMessageToThread` function is exposed through `lib/worker_threads.js` [14](#ref-q1-14)  which imports it from `internal/worker/messaging` [14](#ref-q1-14) . The `process.on('workerMessage')` event is a relatively new addition, introduced in Node.js v22.5.0 and v20.19.0 [15](#ref-q1-15) . Previously, communication between non-parent/child worker threads typically involved `MessageChannel` and `MessagePort` objects [16](#ref-q1-16) .

Wiki pages you might want to explore:
- [Core Runtime Architecture (nodejs/node)](/wiki/nodejs/node#2)
- [Module System Architecture (nodejs/node)](/wiki/nodejs/node#2.4)

View this search on DeepWiki: https://deepwiki.com/search/how-does-nodeworkerthreads-pos_f36a0adf-2459-4416-ad5e-d206656740ce

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/worker_threads.md:479-500`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L479-L500)

```markdown
## `worker_threads.postMessageToThread(threadId, value[, transferList][, timeout])`

<!-- YAML
added:
- v22.5.0
- v20.19.0
-->

> Stability: 1.1 - Active development

* `threadId` {number} The target thread ID. If the thread ID is invalid, a
  [`ERR_WORKER_MESSAGING_FAILED`][] error will be thrown. If the target thread ID is the current thread ID,
  a [`ERR_WORKER_MESSAGING_SAME_THREAD`][] error will be thrown.
* `value` {any} The value to send.
* `transferList` {Object\[]} If one or more `MessagePort`-like objects are passed in `value`,
  a `transferList` is required for those items or [`ERR_MISSING_MESSAGE_PORT_IN_TRANSFER_LIST`][] is thrown.
  See [`port.postMessage()`][] for more information.
* `timeout` {number} Time to wait for the message to be delivered in milliseconds.
  By default it's `undefined`, which means wait forever. If the operation times out,
  a [`ERR_WORKER_MESSAGING_TIMEOUT`][] error is thrown.
* Returns: {Promise} A promise which is fulfilled if the message was successfully processed by destination thread.
```

<a id="ref-q1-2"></a>
### [2] `doc/api/worker_threads.md:499`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L499)

```markdown
* Returns: {Promise} A promise which is fulfilled if the message was successfully processed by destination thread.
```

<a id="ref-q1-3"></a>
### [3] `doc/api/process.md:249-254`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/process.md#L249-L254)

```markdown
* `value` {any} A value transmitted using [`postMessageToThread()`][].
* `source` {number} The transmitting worker thread ID or `0` for the main thread.

The `'workerMessage'` event is emitted for any incoming message send by the other
party by using [`postMessageToThread()`][].
```

<a id="ref-q1-4"></a>
### [4] `doc/api/process.md:249-250`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/process.md#L249-L250)

```markdown
* `value` {any} A value transmitted using [`postMessageToThread()`][].
* `source` {number} The transmitting worker thread ID or `0` for the main thread.
```

<a id="ref-q1-5"></a>
### [5] `doc/api/worker_threads.md:536-539`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L536-L539)

```markdown
  process.on('workerMessage', (value, source) => {
    console.log(`${source} -> ${threadId}:`, value);
    postMessageToThread(source, { message: 'pong' });
  });
```

<a id="ref-q1-6"></a>
### [6] `doc/api/worker_threads.md:490-492`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L490-L492)

```markdown
  [`ERR_WORKER_MESSAGING_FAILED`][] error will be thrown. If the target thread ID is the current thread ID,
  a [`ERR_WORKER_MESSAGING_SAME_THREAD`][] error will be thrown.
* `value` {any} The value to send.
```

<a id="ref-q1-7"></a>
### [7] `lib/internal/errors.js:1918`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/errors.js#L1918)

```javascript
E('ERR_WORKER_MESSAGING_SAME_THREAD', 'Cannot sent a message to the same thread', Error);
```

<a id="ref-q1-8"></a>
### [8] `doc/api/worker_threads.md:496-498`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L496-L498)

```markdown
* `timeout` {number} Time to wait for the message to be delivered in milliseconds.
  By default it's `undefined`, which means wait forever. If the operation times out,
  a [`ERR_WORKER_MESSAGING_TIMEOUT`][] error is thrown.
```

<a id="ref-q1-9"></a>
### [9] `lib/internal/errors.js:1919`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/errors.js#L1919)

```javascript
E('ERR_WORKER_MESSAGING_TIMEOUT', 'Sending a message to another thread timed out', Error);
```

<a id="ref-q1-10"></a>
### [10] `doc/api/worker_threads.md:503-505`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L503-L505)

```markdown
If the target thread has no listener for the `workerMessage` event, then the operation will throw
a [`ERR_WORKER_MESSAGING_FAILED`][] error.
```

<a id="ref-q1-11"></a>
### [11] `lib/internal/errors.js:1917`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/errors.js#L1917)

```javascript
E('ERR_WORKER_MESSAGING_FAILED', 'Cannot find the destination thread or listener', Error);
```

<a id="ref-q1-12"></a>
### [12] `doc/api/worker_threads.md:506-508`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L506-L508)

```markdown
If the target thread threw an error while processing the `workerMessage` event, then the operation will throw
a [`ERR_WORKER_MESSAGING_ERRORED`][] error.
```

<a id="ref-q1-13"></a>
### [13] `lib/internal/errors.js:1916`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/errors.js#L1916)

```javascript
E('ERR_WORKER_MESSAGING_ERRORED', 'The destination thread threw an error while processing the message', Error);
```

<a id="ref-q1-14"></a>
### [14] `lib/worker_threads.js:25-26`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/worker_threads.js#L25-L26)

```javascript
  postMessageToThread,
} = require('internal/worker/messaging');
```

<a id="ref-q1-15"></a>
### [15] `doc/api/process.md:244-247`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/process.md#L244-L247)

```markdown
added:
- v22.5.0
- v20.19.0
-->
```

<a id="ref-q1-16"></a>
### [16] `doc/api/worker_threads.md:1538-1542`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1538-L1542)

```markdown
To create custom messaging channels (which is encouraged over using the default
global channel because it facilitates separation of concerns), users can create
a `MessageChannel` object on either thread and pass one of the
`MessagePort`s on that `MessageChannel` to the other thread through a
pre-existing channel, such as the global one.
```
