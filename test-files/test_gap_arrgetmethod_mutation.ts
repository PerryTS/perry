const a: any = [1,2,3,4];
const original = Array.prototype[Symbol.iterator];
function sum() { let s = 0; for(const x of a) s += x; return s; }
function other() { let i = 0; return {next() {return {done: i >= 2, value: 10 + i++};}}; }
for(const kind of ["own", "prototype", "next"]) {
    const p: any = Object.getPrototypeOf(a.values());
    const originalNext = p.next;
    let s = 0;
    for(const x of a) {
        s += x;
        if (x === 1) {
            if(kind === "own") a[Symbol.iterator] = other;
            if(kind === "prototype") Array.prototype[Symbol.iterator] = other;
            if(kind === "next") p.next = function() { return {done: true}; };
        }
    }
    console.log(kind, s, sum());
    delete a[Symbol.iterator];
    Array.prototype[Symbol.iterator] = original;
    p.next = originalNext;
}
let captures = 0;
a[Symbol.iterator] = function(this: any) {
    const it: any = {next() { return {value: 1, done: false}; }};
    Object.defineProperty(it, "next", {get() {
        captures++;
        let i=0;
        return function() { it.next = () => ({done:true}); return {value: ++i, done:i>4}; };
    }, set(v) {}, configurable:true});
    return it;
};
console.log("captured", sum(), captures);
