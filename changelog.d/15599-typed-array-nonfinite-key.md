A typed array store at a NaN or infinite Number key runs `ToNumber` on the value before it drops the store, as Node does: `ta[NaN] = { valueOf() { … } }` calls `valueOf` once.
