The collection-iterator latch is keyed to Map/Set receivers instead of any `@@iterator` write.

`ITERATOR_PROTOCOL_TOUCHED` previously flipped on any `@@iterator` symbol write anywhere in the process, so an ordinary class merely *defining* `[Symbol.iterator]` disabled the plain-collection iteration lane (#8991) process-wide — which is to say the lane did not run in any realistic program.

Narrowing to registered Map/Set receivers is sound because perry models no `Map.prototype` object to patch: `symbol::get` emulates `Map.prototype[Symbol.iterator]` by binding (#2856), so an own write on the instance is the only override path.
