// Opaque receivers keep these reads on the ordinary method-site path.
const key = {};
const choices: any[] = [new WeakMap(), {}];
const map: any = choices[0];
function get(m: any) { return m.get(key); }
function has(m: any) { return m.has(key); }
function set(m: any, value: number) { return m.set(key, value) === m; }
console.log(set(map, 7), get(map), has(map));
for (let i = 0; i < 100; i++) { get(map); has(map); set(map, 7); }
const proto: any = WeakMap.prototype;
const originalGet = proto.get;
const originalHas = proto.has;
const originalSet = proto.set;
// Allocate after materialization: a new instance must preserve the CLASS word.
const lateChoices: any[] = [new WeakMap(), {}];
const lateMap: any = lateChoices[0];
console.log(lateMap.get === originalGet);
proto.get = function(k: any) { return this === map && k === key ? 19 : -1; };
proto.has = function(k: any) { return this === map && k === key ? false : true; };
proto.set = function(k: any, v: any) { console.log("patched-set", this === map, k === key, v); return this; };
console.log(get(map), has(map), set(map, 9), get(lateMap));
proto.get = originalGet; proto.has = originalHas; proto.set = originalSet;
console.log(get(map), has(map), set(map, 11), get(map));
delete proto.get;
console.log(typeof map.get);
Object.defineProperty(proto, "get", { configurable: true, get() {
    console.log("getter-this", this === map);
    return function(k: any) { return k === key ? 23 : -1; };
}});
console.log(get(map));
Object.defineProperty(proto, "get", { value: originalGet, writable: true, configurable: true });
console.log(get(map));
Object.setPrototypeOf(map, { get(k: any) { return k === key ? 29 : -1; } });
console.log(get(map));
Object.setPrototypeOf(map, proto);
console.log(get(map));
map.get = function(k: any) { return k === key ? 41 : -1; };
console.log(get(map));
delete map.get;
console.log(get(map));
Object.setPrototypeOf(map, new Proxy(proto, { get(target, name, receiver) {
    if (name === "get") { console.log("proxy-get"); return function() { return 43; }; }
    return Reflect.get(target, name, receiver);
}}));
console.log(get(map));
Object.setPrototypeOf(map, proto);
console.log(get(map));
try { originalGet.call({}, key); console.log("bad-brand"); } catch (e) { console.log(e instanceof TypeError); }
Object.setPrototypeOf(map, null);
console.log(typeof map.get);
Object.setPrototypeOf(map, proto);
console.log(get(map));
class Child extends WeakMap<object, number> {}
const children: any[] = [new Child(), {}];
const child: any = children[0];
console.log(set(child, 31), get(child), has(child));
const sets: any[] = [new WeakSet(), {}];
const weakSet: any = sets[0]; weakSet.add(key);
console.log(has(weakSet));
const natives: any[] = [new Map(), new Set()];
natives[0].set(key, 37); natives[1].add(key);
console.log(get(natives[0]), has(natives[0]), has(natives[1]));
