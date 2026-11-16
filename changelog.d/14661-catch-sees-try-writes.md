A `catch` or `finally` block no longer assumes a local still holds a Number
when its `try` body could have overwritten it before throwing: `v + 1` in the
catch yields `NaN` for a `v` set to `undefined` in the try, as in Node,
instead of `undefined` (#12666).
