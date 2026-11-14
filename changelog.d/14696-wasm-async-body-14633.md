`--target web`: inside an `async` function, `Object.keys/values/entries`, `array.join`, `i++`/`--i`, `for...of` over an array and a `catch` block now work (#14633); they printed `undefined`, looped forever or threw `ReferenceError` before.
For...of over a non-array and array destructuring in an async body are still unsupported there and now reject with a `TypeError` naming the gap.
