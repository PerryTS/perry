// Indirect eval of runtime source replaces JSON.stringify; direct calls must
// call the replacement and JSON.parse must stay the builtin.
const value = { a: 1, b: [true, null] };
function encode(v: unknown): string {
    return JSON.stringify(v);
}
console.log(encode(value), JSON.stringify(value, null, 1).length);
const indirect = eval;
const source = process.argv[2] || "JSON.stringify = function (v) { return 'S(' + typeof v + ')'; };";
indirect(source);
console.log(encode(value), JSON.stringify([1]), JSON.parse('{"x":2}').x);
