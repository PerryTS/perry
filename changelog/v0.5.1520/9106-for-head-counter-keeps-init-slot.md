### Fixed

- Classic `for` heads shaped `for (let i = 0, len = arr.length; i < len; i++)` regained their versioned packed-loop fast clones: when hoisting the tail declarators around a literal-initialized counter is provably unobservable, the counter stays in the loop's own init slot the counted-loop matchers key on, while order-observable heads keep the #9062 source-order lowering.
