`instanceof` no longer segfaults on short inline strings, and `Object.create(proto).constructor`
returns the real constructor. The receiver is now resolved per value kind rather than assumed to be a
heap pointer, with the prototype and class-registry paths updated to match.

The string crash took down ajv, and with it every fastify schema route; the `constructor` defect
crashed lodash's `isEqual`. `new EventEmitter() instanceof EventEmitter` is fixed as a direct
consequence; the `extends EventEmitter` subclass shape is tracked separately.
