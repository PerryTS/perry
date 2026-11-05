function fingerprint(label: string, value: any): void {
  const descriptors: any = Object.getOwnPropertyDescriptors(value);
  const keys = Reflect.ownKeys(descriptors);
  const rows: string[] = [];
  for (const key of keys) {
    const d: any = descriptors[key];
    const kind = 'value' in d ? 'data:' + typeof d.value + ':' + (typeof d.value === 'object' || typeof d.value === 'function' ? '-' : String(d.value)) + ':' + d.writable : 'accessor:' + typeof d.get + ':' + typeof d.set;
    rows.push(String(key) + ':' + kind + ':' + d.enumerable + ':' + d.configurable);
    const single: any = Object.getOwnPropertyDescriptor(value, key);
    if (single.enumerable !== d.enumerable || single.configurable !== d.configurable || single.writable !== d.writable || !Object.is(single.value, d.value) || single.get !== d.get || single.set !== d.set) throw new Error('descriptor mismatch');
  }
  console.log(label + ' ' + rows.join('|'));
}
let calls = 0;
const ordinary: any = {z: 1, '20': 20, '3': 3, '03': 30, a: -0};
Object.defineProperty(ordinary, 'hidden', {value: undefined});
Object.defineProperty(ordinary, 'computed', {enumerable: true, configurable: true, get() {calls++; return 9;}, set(v: any) {calls++;}});
Object.defineProperty(ordinary, 'setter', {enumerable: false, set(v: any) {calls++;}});
ordinary[Symbol('public')] = 7;
Object.defineProperty(ordinary, Symbol('accessor'), {get() {calls++; return 12;}});
fingerprint('ordinary', ordinary);
fingerprint('frozen', Object.freeze({first: 1, second: 2}));
fingerprint('sealed', Object.seal({first: 1}));
fingerprint('null-prototype', Object.assign(Object.create(null), {a: 1, b: 2}));
const array: any = [1, , undefined]; array.extra = 4;
Object.defineProperty(array, '1', {enumerable: true, configurable: true, get() {calls++; return 8;}});
Object.defineProperty(array, 'extra', {writable: false});
fingerprint('array', array);
fingerprint('frozen-array', Object.freeze([1, 2]));
fingerprint('string', 'abc');
fingerprint('boxed-string', new String('ab'));
fingerprint('typed-array', new Uint8Array([3, 4]));
fingerprint('date', Object.assign(new Date(1), {extra: 4}));
fingerprint('regexp', /ab/g);
class Example { #hidden = 7; value = 4; get computed() {calls++; return 8;} static marker = 6; }
fingerprint('class-instance', new Example());
fingerprint('class-prototype', Example.prototype);
fingerprint('class-constructor', Example);
const example = (a: any, b: any) => a;
(example as any).extra = 9;
fingerprint('function', example);
const log: string[] = [];
const proxy = new Proxy({a: 1, b: 2}, {
  ownKeys(target) {log.push('keys'); return Reflect.ownKeys(target);},
  getOwnPropertyDescriptor(target, key) {log.push(String(key)); return Reflect.getOwnPropertyDescriptor(target, key);}
});
const proxyDescriptors = Object.getOwnPropertyDescriptors(proxy);
console.log('proxy ' + log.join(',') + ':' + JSON.stringify(proxyDescriptors));
console.log('reflection-getters ' + calls);
const parent: any = {first: {}, tail: 2, last: 3};
Object.defineProperty(parent.first, 'trigger', {enumerable: true, get() {
  delete parent.tail; Object.defineProperty(parent, 'last', {enumerable: true, get() {calls++; return 11;}}); parent.added = 4; return 1;
}});
console.log('clone-mutated ' + JSON.stringify(structuredClone(parent)) + ':' + calls);
const bag: any = {first: {enumerable: true, get value() {bag.second = {value: 9, enumerable: true}; bag.added = {value: 4}; return 1;}}, second: {value: 2, enumerable: true}};
console.log('defineProperties ' + JSON.stringify(Object.defineProperties({}, bag)));
