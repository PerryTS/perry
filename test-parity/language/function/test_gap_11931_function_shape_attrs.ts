function descriptor(label: string, fn: any, key: string) {
  const d = Object.getOwnPropertyDescriptor(fn, key);
  if (!d) { console.log(label, key, 'absent'); return; }
  const value = typeof d.value === 'function' ? 'callable' : d.value;
  console.log(label, key, JSON.stringify([value, d.writable, d.enumerable, d.configurable, typeof d.get, typeof d.set]));
}
function snapshot(label: string, fn: any) {
  descriptor(label, fn, 'name'); descriptor(label, fn, 'length');
  console.log(label, 'read', fn.name, fn.length);
  console.log(label, 'keys', Object.keys(fn).join(','));
  const keys: string[] = []; for (const k in fn) keys.push(k);
  console.log(label, 'for-in', keys.join(','));
}
function exercise(label: string, fn: any) {
  snapshot(label + '-initial', fn);
  Object.defineProperty(fn, 'name', {value:'renamed', writable:true, enumerable:true});
  console.log(label, 'reflect-length', Reflect.defineProperty(fn, 'length', {writable:true, enumerable:true}));
  snapshot(label + '-redefined', fn);
  fn.name = 'assigned'; fn.length = 12;
  snapshot(label + '-assigned', fn);
  Object.defineProperty(fn, 'name', {value:undefined});
  descriptor(label + '-undefined', fn, 'name');
  console.log(label, 'delete', delete fn.name, delete fn.length);
  snapshot(label + '-deleted', fn);
  Object.defineProperty(fn, 'name', {value:'again', configurable:true});
  Object.defineProperty(fn, 'length', {value:7, configurable:true});
  Object.defineProperty(fn, 'custom', {get(){return 23}, configurable:true, enumerable:true});
  Object.freeze(fn);
  snapshot(label + '-frozen', fn); descriptor(label, fn, 'custom');
  console.log(label, 'custom', fn.custom, 'frozen', Object.isFrozen(fn));
  console.log(label, 'rejected', Reflect.defineProperty(fn, 'name', {value:'wrong'}), Reflect.deleteProperty(fn, 'name'));
}
function user(a: any, b: any) { return a; }
function target(a: any, b: any, c: any) { return a; }
class Klass { static method() {return 1} }
const O: any = Object;
const method: any = O.keys;
exercise('builtin-method', method);
exercise('user', user);
exercise('bound', target.bind(null, 1));
exercise('class', Klass);
const M: any = Map;
snapshot('map', M);
console.log('map-delete', delete M.name);
console.log('map-deleted-read', M.name);
descriptor('map-deleted', M, 'name');
Object.seal(M);
descriptor('map-sealed', M, 'name'); descriptor('map-sealed', M, 'length');
console.log('map-sealed-state', Object.isSealed(M));
descriptor('static', O, 'create'); descriptor('static', O, 'hasOwn');
Object.defineProperty(O, 'create', {enumerable:true, writable:false});
descriptor('static-redefined', O, 'create');
console.log('static-enumerated', Object.keys(O).includes('create'));
console.log('static-delete', delete O.create);
descriptor('static-deleted', O, 'create');
const A: any = Array;
Object.freeze(A); descriptor('array-frozen', A, 'isArray');
const f: any = function sealme(x: any) {};
Object.defineProperty(f, 'custom', {get(){return 17}, configurable:true});
Object.seal(f); descriptor('user-sealed', f, 'name'); descriptor('user-sealed', f, 'custom');
console.log('user-sealed-read', f.custom, Object.isSealed(f));

const empty: any = function attrOnly(x: any) {};
Object.defineProperty(empty, 'extra', {enumerable:true, configurable:true});
descriptor('new-attrs-only', empty, 'extra');
Object.defineProperty(empty, 'name', {get(){return 'getter-name'}, configurable:true});
Object.defineProperty(empty, 'name', {enumerable:true});
descriptor('retained-accessor', empty, 'name'); console.log('retained-accessor-read', empty.name);
Object.defineProperty(empty, 'name', {get:undefined});
descriptor('empty-accessor', empty, 'name');
Object.defineProperty(empty, 'name', {value:undefined, writable:true});
descriptor('accessor-to-data', empty, 'name');
Object.freeze(empty);
console.log('nonextensible-reject', Reflect.defineProperty(empty, 'newKey', {value:1}));
for (let i = 0; i < 80; i++) {
    const fn: any = (x: any) => x;
    Object.defineProperty(fn, 'name', {value:'function-' + i, enumerable:true});
    const trash: any[] = [];
    for (let j = 0; j < 20; j++) trash.push({j});
    if (i === 0 || i === 79) snapshot('gc-' + i, fn);
}

function rebind(a: any, b: any, c: any) {}
Object.defineProperty(rebind, "name", {value:undefined});
snapshot("bound-non-string-name", rebind.bind(null));
delete (rebind as any).length;
snapshot("bound-deleted-length", rebind.bind(null));

function receiver() {}
const receiverAlias: any = receiver;
Object.defineProperty(receiverAlias, 'name', {
    get: function () { return this === receiverAlias ? 'self' : 'wrong-receiver'; },
    set: function (value: any) { this.marker = value; },
    configurable: true, enumerable: true,
});
receiverAlias.name = 'updated';
console.log('function-accessor-receiver', receiverAlias.name, receiverAlias.marker);
Object.freeze(receiverAlias);
descriptor('frozen-accessor-receiver', receiverAlias, 'name');

const symbolFunctions: any[] = [Object.values, function symbolUser() {}, (function () {}).bind(null), class SymbolClass {}];
for (let i = 0; i < symbolFunctions.length; i++) {
    const fn: any = symbolFunctions[i];
    const key = Symbol('function-key');
    Object.defineProperty(fn, key, {value: 11, writable: true, enumerable: true, configurable: true});
    const before = Object.getOwnPropertyDescriptor(fn, key);
    console.log('function-symbol-data', i, fn[key], before.writable, before.enumerable, before.configurable, Object.getOwnPropertySymbols(fn).length);
    Reflect.defineProperty(fn, key, {get: function () { return this === fn ? 22 : -1; }});
    const accessor = Object.getOwnPropertyDescriptor(fn, key);
    console.log('function-symbol-accessor', i, fn[key], typeof accessor.get, accessor.enumerable, accessor.configurable);
    console.log('function-symbol-delete', i, delete fn[key], Object.getOwnPropertyDescriptor(fn, key) === undefined);
    Object.defineProperty(fn, key, {value: 33, writable: true, configurable: true});
    Object.freeze(fn);
    const frozen = Object.getOwnPropertyDescriptor(fn, key);
    console.log('function-symbol-freeze', i, fn[key], frozen.writable, frozen.enumerable, frozen.configurable, Reflect.defineProperty(fn, key, {value:44}), Reflect.defineProperty(fn, key, {value:33}));
}

function sameValue(a: any) {}
Object.freeze(sameValue);
console.log('function-frozen-same-value', Reflect.defineProperty(sameValue, 'length', {value:1}), Reflect.defineProperty(sameValue, 'length', {value:2}));

// The lazy intrinsic-name snapshot must survive later changes to the target.
function snapshotTarget(a: any, b: any) {}
const snapshotAlias: any = snapshotTarget;
const savedBinding: any = snapshotAlias.bind(null);
Object.defineProperty(snapshotAlias, "name", { value: "later" });
Object.defineProperty(snapshotAlias, "length", { value: 9 });
console.log("bind-intrinsic-snapshot", savedBinding.name, savedBinding.length);
let nameGetterCalls = 0;
Object.defineProperty(snapshotAlias, "name", {
  get() { nameGetterCalls++; return "getter-name"; }, configurable: true,
});
const getterBinding: any = snapshotAlias.bind(null);
console.log("bind-name-getter", nameGetterCalls, getterBinding.name, nameGetterCalls);
