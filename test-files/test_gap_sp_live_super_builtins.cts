const { EventEmitter } = require('node:events');
class A extends Array { constructor(...args) { super(...args); this.own = 3; } }
const first = new A(1, 2);
console.log('array', Array.isArray(first), first.join(','), first.own);
class Other { constructor(...args) { this.args = args.join(','); this.target = new.target.name; } }
Object.setPrototypeOf(A, Other);
const second = new A(4, 5);
console.log('other', Array.isArray(second), second.args, second.own, second.target);
class B extends Other { constructor(...args) { super(...args); this.own = 6; } }
Object.setPrototypeOf(B, Array);
const third = new B(7, 8);
console.log('to-array', Array.isArray(third), Array.prototype.join.call(third, ','), third.own, third instanceof B);
class E extends Error { constructor(v) { super(v); this.own = 7; } }
Object.setPrototypeOf(E, Other);
const error = new E('hi');
console.log('from-error', error.args, error.own, Object.hasOwn(error, 'message'));
class M extends Map { constructor(...args) { super(...args); this.own = 8; } }
Object.setPrototypeOf(M, Other);
const map = new M('x');
console.log('from-map', map.args, map.own, Object.hasOwn(map, 'size'));

class Target {}
const reflected = Reflect.construct(B, [9, 10], Target);
console.log('reflect-array', Array.isArray(reflected), Array.prototype.join.call(reflected, ','), reflected.own, Object.getPrototypeOf(reflected) === Target.prototype);

class P extends Promise { constructor(value) { super(value); this.own = 2; } }
Object.setPrototypeOf(P, Other);
const p = new P('x');
console.log('from-promise', p.args, p.own);
try { Promise.prototype.then.call(p, () => {}); console.log('branded'); } catch (e) { console.log('promise-brand', e instanceof TypeError); }
class DP extends Promise {}
Object.setPrototypeOf(DP, Other);
console.log('default-promise', new DP('y').args);

class Empty {}
class RelinkedArray extends Array {}
const genuine = new RelinkedArray(2);
Object.setPrototypeOf(RelinkedArray, Empty);
const ordinary = new RelinkedArray();
ordinary.push(1); ordinary[3] = 9; genuine[3] = 9;
console.log('array-identity', Array.isArray(ordinary), ordinary.length, Array.isArray(genuine), genuine.length);

// Native heritage is the actual imported constructor, including default chains.
class Bus extends EventEmitter { emit(ev, ...args) { return super.emit(ev, ...args); } }
class Logged extends Bus { emit(ev, value) { return super.emit(ev, value); } }
console.log('native-parent', Object.getPrototypeOf(Bus) === EventEmitter);
const logged = new Logged();
let received = '';
logged.on('x', value => { received = 'got:' + value; });
logged.emit('x', 5);
console.log('native-base', received);
Object.setPrototypeOf(Bus, Other);
const nativeRelinked = new Logged(42);
console.log('native-relink', nativeRelinked.args, Object.hasOwn(nativeRelinked, '_events'));
class NaturalMap extends Map {}
console.log('map-parent', Object.getPrototypeOf(NaturalMap) === Map);
const naturalMap = new NaturalMap(); naturalMap.set('k', 9);
console.log('map-base', naturalMap.get('k'));
