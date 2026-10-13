`node:events` closes the five remaining node-suite divergences in the module
(70 PASS / 0 FAIL, up from 65/5).

`EventEmitterAsyncResource` now matches node's contract: `emitDestroy()` returns
`undefined` like node's `emitDestroy() { this.#asyncResource.emitDestroy(); }`
(the inherited `AsyncResource.prototype.emitDestroy` still returns its
receiver), and a direct `new EventEmitterAsyncResource()` whose `options.name`
is not a string — `undefined`, `{}`, `null` — throws
`TypeError [ERR_INVALID_ARG_TYPE]` from `validateString(options?.name,
'options.name')` instead of succeeding. The `new.target.name` default still
applies to subclasses.

`removeAllListeners` now branches on the caller's argument count, as node's
`arguments.length === 0` does: `removeAllListeners()` still drains every event,
while `removeAllListeners(undefined)` removes the coerced `"undefined"` key
alone instead of taking the `"null"` key with it. `once` wrappers are born on
the property-read shape their own bag implies, so a `rawListeners(...)` entry
answers `.listener` no matter which plain listeners share its list — previously
`rawListeners("x").filter(l => l.listener === h)[0]` could come back `undefined`
and `listenerCount("x", undefined)` returned the total instead of the wrapper's
count.

The `MaxListenersExceededWarning` leak check now goes through the JS-visible
`process.emitWarning`, so replacing that property intercepts the warning as it
does in node, and it names the emitter as `[EventEmitter]` — node's
`inspect(target, {depth: -1})` — instead of dumping the emitter object. An
`Error` passed to `process.emitWarning` keeps its own `name`, `code`, `detail`
and `message`, prints as `(node:<pid>) [code] Name: message` with a `detail`
line, and reaches `warning` listeners as the very same object rather than a
rebuilt one; the positional `type`/`code` are validated and ignored exactly as
node ignores them for an Error. String warnings are unchanged.

Known follow-up: `util.inspect` / `console.dir` still clamp a negative `depth`
to 0 (`builtins/console.rs`'s `decode_dir_depth_option`), so
`util.inspect(x, {depth: -1})` renders the object instead of `[Constructor]`;
the leak warning computes node's value itself until that shared surface gets its
own change.
