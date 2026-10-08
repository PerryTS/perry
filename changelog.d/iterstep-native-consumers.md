Compiled synchronous for-of and array destructuring consume built-in iterator steps as native value/done pairs. Manual next calls retain fresh result objects. Array (including Buffer-backed Uint8Array), Map, Set and String advances share one emitter, replacing the per-iterator result cache. Iterator records capture next once and close still resolves the current return method.

Buffer-backed iterators inherit the Array iterator prototype so captured-next consumers use the ordinary protocol.
