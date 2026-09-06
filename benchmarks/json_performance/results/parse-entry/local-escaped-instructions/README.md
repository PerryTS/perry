A targeted local instruction diagnostic follows the qualified 2.52% escaped
stringify regression. Retired process instructions increase only 0.0473%
(three fresh processes per arm, 84 calls/two warmups). This does not support
attributing the CPU change to a proportional increase in instruction count.
The next investigation should compare hot-loop execution and generated code;
these counters do not prove a particular layout, cache or allocator cause.
