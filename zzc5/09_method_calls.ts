// Benchmark: Method call overhead
// Measures virtual dispatch performance
const ITERATIONS = 10000000;

class Counter {
    value: number;
    constructor() {
        this.value = 0;
    }
    increment(): void {
        this.value = this.value + 1;
    }
    get(): number {
        return this.value;
    }
}

const counter = new Counter();
const start = Date.now();
for (let i = 0; i < ITERATIONS; i++) {
    counter.increment();
}
const elapsed = Date.now() - start;

console.log("method_calls:" + elapsed);
console.log("value:" + counter.get());
