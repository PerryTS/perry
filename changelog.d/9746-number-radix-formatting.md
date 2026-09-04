Fix non-decimal `Number.prototype.toString(radix)` digit selection. Large
integers now zero-fill digits beyond the double's precision and subtract
remainders before dividing, matching Node for cases such as
`(1e21).toString(36)` and `(9007199254740994).toString(3)`. Fractional conversion
also rounds the final digit when the residual is within the stopping tolerance,
fixing `(0.1).toString(36)`.

Runtime regressions and an end-to-end fixture cover all radices, both signs,
the 2^53 boundary, large exponents, fractional values, dynamic calls, and boxed
numbers. The original formatter fails the new large-integer regression.
