// Test generic classes
// Phase 3 test: verify class type parameters are extracted

class Box<T> {
    value: T;

    constructor(v: T) {
        this.value = v;
    }

    get(): T {
        return this.value;
    }

    set(v: T): void {
        this.value = v;
    }
}

// Create boxes with different types
let numBox = new Box<number>(42);
let strBox = new Box<string>("hello");

console.log(numBox.get());
console.log(strBox.get());

numBox.set(100);
console.log(numBox.get());

// Test multiple type parameters
class Pair<K, V> {
    key: K;
    value: V;

    constructor(k: K, v: V) {
        this.key = k;
        this.value = v;
    }

    getKey(): K {
        return this.key;
    }

    getValue(): V {
        return this.value;
    }
}

let pair = new Pair<string, number>("age", 30);
console.log(pair.getKey());
console.log(pair.getValue());

console.log("Generic class tests passed!");
