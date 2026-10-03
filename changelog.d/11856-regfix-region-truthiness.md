A value only tested for truthiness (a conditional's test, a `!` operand) no
longer asks a loop region for a Number lane, directly or through an
accumulator's value flow. A string field tested that way failed the region's
value test on every iteration: #10495 `own3` 246 -> 228 instructions per
operation.
