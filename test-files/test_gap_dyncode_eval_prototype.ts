class Counter {
    value = 10;
    read() { return this.value; }
}
const counter = new Counter();
const source = process.argv[2] || "Counter.prototype.read = function () { return 42; };";
eval(source);
console.log(counter.read());
