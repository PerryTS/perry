// A direct eval of runtime source replaces Math.max. The compiler cannot see
// the write; every direct Math.max call after it must call the replacement
// (the namespace's shadow bit routes it), and calls before it the builtin.
function biggest(a: number, b: number): number {
    return Math.max(a, b);
}
const xs = [3, 9, 4];
console.log(biggest(1, 2), Math.max(...xs), Math.min(5, 6));
const source = process.argv[2] || "Math.max = function (a, b) { return 'max:' + a + ',' + b; };";
eval(source);
console.log(biggest(1, 2), Math.max(...xs), Math.min(5, 6));
let out = "";
for (let i = 0; i < 3; i++) out += biggest(i, 10 - i) + ";";
console.log(out);
// The callee is read before the arguments are evaluated.
const restore = process.argv[3] || "Math.max = function () { return 'second'; };";
console.log(Math.max(1, (eval(restore), 2)), Math.max(1, 2));
