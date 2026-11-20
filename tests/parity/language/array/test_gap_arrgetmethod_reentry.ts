const a: any = [1,2,3,4];
const values = Array.prototype.values;
const log: string[] = [];
function collect() { let sum = 0; for(const x of a) sum += x; return sum; }
Object.defineProperty(a, Symbol.iterator, {configurable:true, get() {
    log.push("get:" + (this === a));
    Object.defineProperty(a, Symbol.iterator, {value: 12, configurable:true, writable:true});
    return function(this: any) {log.push("call:" + (this === a)); return values.call(this);};
}});
console.log(collect());
try { collect(); } catch(e: any) {console.log("second", e instanceof TypeError);}
a[Symbol.iterator] = new Proxy(function(this: any) {return values.call(this);}, {apply(target, recv, args) {
    log.push("apply:" + (recv === a) + ":" + args.length);
    return Reflect.apply(target, recv, args);
}});
console.log(collect());
a[Symbol.iterator] = (function(this: any) {log.push("bound:" + (this === a)); return values.call(this);}).bind(a);
console.log(collect());
a[Symbol.iterator] = function() {
    let i=0;
    const result: any = [];
    result.next = function(this: any) {log.push("array-next:" + (this === result)); return {value: ++i, done:i>2};};
    return result;
};
console.log(collect());
Object.setPrototypeOf(a, null);
console.log(collect());
console.log(log.join(","));
