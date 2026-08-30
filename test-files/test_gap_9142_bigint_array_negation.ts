// #9142: unary minus on an indexed BigInt must preserve BigInt semantics.
// The old lowering treated array elements as Numbers, rounding values above
// 2^53 through f64 before negating them.

const inferred = [2n ** 64n];
console.log(-inferred[0]);
console.log(typeof -inferred[0]);

const annotated: bigint[] = [2n ** 100n + 12345n];
console.log(-annotated[0]);
console.log(typeof -annotated[0]);
