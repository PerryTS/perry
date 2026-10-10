const trace: string[] = [];
const a: any = [1, 2, 3, 4];
function make(this: any) {
    trace.push("call:" + (this === a));
    let i = 0;
    const it: any = {};
    Object.defineProperty(it, "next", { get() {
        trace.push("get-next:" + (this === it));
        return function(this: any) {
            trace.push("step:" + (this === it));
            return { value: ++i, done: i > 4 };
        };
    }});
    return it;
}
function run() { let sum = 0; for (const x of a) sum += x; console.log(sum); }
a[Symbol.iterator] = make;
run();
delete a[Symbol.iterator];
const original = Array.prototype[Symbol.iterator];
Array.prototype[Symbol.iterator] = make;
run();
const [x,y] = a;
console.log(x, y);
Array.prototype[Symbol.iterator] = original;
const p: any = Object.getPrototypeOf(a.values());
const next = p.next;
let seen: any;
Object.defineProperty(p, "next", { configurable: true, get() {
    trace.push("proto-get-next");
    return function(this: any) { seen = this; return next.call(this); };
}});
run();
console.log(seen !== a);
Object.defineProperty(p, "next", { value: next, configurable: true, writable: true });
console.log(trace.join(","));
