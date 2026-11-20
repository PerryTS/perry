// A direct eval of runtime source replaces a class's prototype method; call
// sites the compiler resolved to that class must call the replacement.
class Counter {
    value = 10;
    read() {
        return this.value;
    }
}
function readIt(c: Counter): number {
    return c.read();
}
const counter = new Counter();
console.log(readIt(counter), counter.read());
const source = process.argv[2] || "Counter.prototype.read = function () { return this.value + 32; };";
eval(source);
console.log(readIt(counter), counter.read(), new Counter().read());
// new Function sees only globals: it reaches a class once the program puts
// the class there.
(globalThis as any).Counter = Counter;
const body = process.argv[3] || "globalThis.Counter.prototype.read = function () { return -1; };";
new Function(body)();
console.log(readIt(counter), counter.read());
