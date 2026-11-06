// The facts call the iterator's `return` inactive, yet destructuring runs it (a known false
// inactive claim, CXCHK0 findings, accepted witness language/array/test_gap_10086). With
// PERRY_FACTS_DCE=1 the body is a trap: why removal is off by default.
let closed = 0;
const closing: any = {};
closing[Symbol.iterator] = function () {
let i = 0;
return { next: function () { i++; return { done: false, value: i }; },
return: function () { closed++; return { done: true }; } };
};
const [z1] = closing;
console.log(z1, closed);
