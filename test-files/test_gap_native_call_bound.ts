const call = Function.prototype.call;
const apply = Function.prototype.apply;
const get = call.bind(WeakMap.prototype.get);
const set = call.bind(WeakMap.prototype.set);
const has = call.bind(WeakMap.prototype.has);
const key = {};
const map = new WeakMap();
console.log(set(map, key, 42) === map, get(map, key), has(map, key), has(map, {}));
for (const receiver of [{}, new Map(), new WeakSet(), null, 1, undefined]) {
    for (const method of [get, set, has]) {
        try { method(receiver, key, 9); console.log('bad'); }
        catch (e) { console.log(e instanceof TypeError); }
    }
}
const originalGet = WeakMap.prototype.get;
WeakMap.prototype.get = function (_key) { return 77; };
console.log(get(map, key), Reflect.apply(Reflect.get(WeakMap.prototype, "get"), map, [key]));
WeakMap.prototype.get = originalGet;
const bound = get.bind({ ignored: true }, map);
const twice = bound.bind(null, key);
console.log(bound(key), twice(), get.length, set.length);
console.log(apply.bind(originalGet)(map, [key]));
console.log(get(map) === undefined, has(map) === false);
console.log(set(map, key) === map, get(map, key) === undefined);
const push = Array.prototype.push;
const pushApply = apply.bind(push);
const a = [];
console.log(push.apply(a, [1]), pushApply(a, [2, 3]), a.join(','));
console.log(pushApply(a, [4, 5, 6, 7, 8]), a.join(','));
const list = { length: 1, get 0() { console.log('getter'); return 9; } };
console.log(pushApply(a, list), a.join(','));
const sparse = new Array(1);
console.log(pushApply(a, sparse), a[a.length - 1] === undefined);
const obj = { length: 0 };
console.log(push.apply(obj, [11]), obj[0], obj.length);
console.log(push.apply(a, null), push.apply(a, undefined));
try { push.apply(a, 1); console.log('bad list'); }
catch (e) { console.log(e instanceof TypeError); }
