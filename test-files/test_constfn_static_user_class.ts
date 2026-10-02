// Run executable output with PERRY_CONSTFN_SHAPE=0/1 and moving-GC stress.
// Both closure fields must use the current instance after later effects move it.
function later(): number {
    let sum = 0;
    for (let i = 0; i < 128; i++) {
        const temporary = { value: i };
        sum += temporary.value;
    }
    return sum;
}
class Base {
    value = 0;
    method = () => this.value;
    constructor(value: number) {
        this.value = value;
        later();
    }
}
class Child extends Base {
    next = () => this.value + 1;
    effect = later();
    constructor(value: number) {
        super(value);
        this.value = value * 2;
        later();
    }
}
const first = new Child(11);
const second = new Child(23);
console.log(first.method(), second.method(), first.next(), second.next());
console.log(first.method !== second.method, first.effect, second.effect);
second.method = () => 99;
console.log(first.method(), second.method());
delete (first as any).next;
console.log(typeof first.next, second.next());
Object.defineProperty(second, "next", { get: () => () => 123 });
console.log(second.next());
class Replacement {
    method = () => 1;
    constructor() {
        return { method: () => 37 } as any;
    }
}
console.log(new Replacement().method());
