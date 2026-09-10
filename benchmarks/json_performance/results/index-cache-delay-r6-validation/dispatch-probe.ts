function read(a: any, i: any): any { return a[i]; }
function field(a: any, i: any): any { return a[i].id; }
const dense: any = [{id: 3}, {id: 8}];
console.log("dense", field(dense, 0), field(dense, 1), read(dense, 9));
const shaped: any = {0: {id: 7}, 1: {id: 11}, length: 2};
console.log("shape", field(shaped, 0), field(shaped, 1), read(shaped, 2));
class Rows extends Array<any> {}
const sub: any = new Rows(); sub.push({id: 17}); sub.push({id: 19});
console.log("sub", field(sub, 0), field(sub, 1), read(sub, 2));
let count = 0;
const getter: any = [{id: 1}];
Object.defineProperty(getter, "0", {get() {count++; return {id: 23};}, configurable: true});
console.log("indexgetter", field(getter, 0), field(getter, 0), count);
Object.defineProperty(shaped, "1", {get() {count++; return {id: 29};}, configurable: true});
console.log("shapegetter", field(shaped, 1), count);
const record: any = {get id() {count++; return 31;}};
console.log("fieldgetter", field([record], 0), count);
const typed: any = new Int32Array([5, -7, 9]);
console.log("typed", read(typed, 0), read(typed, 1), read(typed, -1), read(typed, 1.5), read(typed, 3));
const view: any = new Float64Array(new ArrayBuffer(32), 8, 2); view[0] = 2.5;
console.log("view", read(view, 0), read(view, 2));
const keys: any = {"-1": {id: 37}, "1.5": {id: 41}, "key": {id: 43}};
console.log("keys", field(keys, -1), field(keys, 1.5), field(keys, "key"));
const key: any = {toString() {count++; return "0";}};
console.log("keycoercion", field(dense, key), count);
const hole: any = [{id: 1}]; delete hole[0];
console.log("hole", read(hole, 0));
Object.defineProperty(Array.prototype, "0", {get() {count++; return {id: 47};}, configurable: true});
console.log("inherited", field(hole, 0), count, field(dense, 0));
delete Array.prototype[0];
console.log("restored", read(hole, 0), field(dense, 0));
