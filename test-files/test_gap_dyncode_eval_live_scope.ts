function run(source: string) {
    let value = 3;
    const get = eval(source);
    value = 8;
    console.log(get());
    get(12);
    console.log(value);
}
run(process.argv[2] || "(function (next) { if (next !== undefined) value = next; return value; })");
const pieces = [process.argv[3] || "Array.prototype.join = function () { return 'spread-eval'; }", 99];
eval(...pieces);
console.log([1, 2].join(','));
const thisSource = process.argv[4] || "this";
console.log(eval(thisSource) === this);
function receiver(source: string) { return eval(source) === this; }
console.log(receiver.call({ value: 1 }, thisSource));
