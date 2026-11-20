`--target web`: the global `isNaN` and `isFinite` did not apply ToNumber, so
`isNaN("abc")` and `isNaN(undefined)` were `false` and `isFinite("1")` was
`false`. They used the bridges of the non-coercing `Number.isNaN` /
`Number.isFinite`; they now have their own.
