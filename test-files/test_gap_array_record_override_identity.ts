function scan(a: any): string { let out = ""; for (const x of a) out += x; return out; }
const values = Array.prototype[Symbol.iterator];
const own: any = [1,2,3];
own[Symbol.iterator] = values.bind([7,8]);
console.log("bound-own", scan(own));
const family: any = Object.getPrototypeOf([].values());
const next = family.next;
const other: any = values.call([9,10]);
family.next = next.bind(other);
console.log("bound-next", scan([1,2,3]));
family.next = next;
const method = function() { return values.call([4,5]); };
delete (Array.prototype as any)[Symbol.iterator];
(Object.prototype as any)[Symbol.iterator] = method;
console.log("inherited-after-delete", scan([1,2,3]));
delete (Object.prototype as any)[Symbol.iterator];
try { scan([1,2,3]); console.log("delete-missed"); }
catch (e) { console.log("delete-throws", e instanceof TypeError); }
Array.prototype[Symbol.iterator] = values;

