// Test static class members

class Counter {
    static count: number = 0;
    static increment(): number {
        Counter.count = Counter.count + 1;
        return Counter.count;
    }
    static decrement(): number {
        Counter.count = Counter.count - 1;
        return Counter.count;
    }
    static reset(): void {
        Counter.count = 0;
    }
}

// Test static field access
console.log(Counter.count);  // 0

// Test static method calls
console.log(Counter.increment());  // 1
console.log(Counter.increment());  // 2
console.log(Counter.increment());  // 3
console.log(Counter.count);  // 3

console.log(Counter.decrement());  // 2
console.log(Counter.count);  // 2

Counter.reset();
console.log(Counter.count);  // 0

// Test static field assignment
Counter.count = 100;
console.log(Counter.count);  // 100

// Test another class with statics
class Config {
    static debug: boolean = true;
    static version: number = 1;

    static getVersion(): number {
        return Config.version;
    }
}

console.log(Config.debug);    // true (should print 1)
console.log(Config.version);  // 1
console.log(Config.getVersion());  // 1

Config.version = 2;
console.log(Config.getVersion());  // 2

console.log("Static member tests passed!");
