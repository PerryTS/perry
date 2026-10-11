// Arguments forwarding must preserve mapping, identity, descriptors and overrides.
function sum(this: any, ...xs: any[]) { return `${this?.tag}:${xs.join(',')}`; }
function forward(this: any) { return (() => sum.apply(this, arguments as any))(); }
console.log('forward', (forward as any).call({tag: 'ok'}, 1, 2, 3));
function sloppy(a: any) {
  (arguments as any).extra = true;
  a = 8;
  console.log('mapped', arguments[0]);
  arguments[0] = 9;
  console.log('alias', a);
  return sum.apply({tag: 'mapped'}, arguments as any);
}
console.log(sloppy(1));
function strict(a: any) {
  'use strict';
  a = 8;
  arguments[0] = 9;
  console.log('unmapped', a, arguments[0]);
  return arguments;
}
const escaped = strict(1);
console.log('escape', escaped.length, escaped[0], Object.keys(escaped).join(','));
console.log('proto', Object.getPrototypeOf(escaped) === Object.prototype);
console.log('inherited-in', 'toString' in escaped, 'hasOwnProperty' in escaped, 'constructor' in escaped);
(escaped as any).constructor = function Other() {};
console.log('own-constructor-proto', Object.getPrototypeOf(escaped) === Object.prototype);
function edited() {
  Object.defineProperty(arguments, '0', {get() { return 12; }, configurable: true});
  delete arguments[1];
  arguments.length = 1;
  return sum.apply({tag: 'edited'}, arguments as any);
}
console.log((edited as any)(1, 2));
function inheritedIndex() {
  delete arguments[0];
  Object.setPrototypeOf(arguments, {0: 71});
  arguments.length = 1;
  return sum.apply({tag: 'inherited'}, arguments as any);
}
console.log((inheritedIndex as any)(1, 2));
const savedApply = Function.prototype.apply;
Function.prototype.apply = function(this: any, receiver: any, xs: any) {
  console.log('override', Array.isArray(xs), Object.prototype.toString.call(xs), xs.length);
  return 'patched';
};
console.log('changed', (forward as any).call({tag: 'x'}, 4, 5));
Function.prototype.apply = savedApply;
console.log('restored', (forward as any).call({tag: 'r'}, 6));
function spread(this: any) { return sum.call(this, ...arguments); }
console.log('call', (spread as any).call({tag: 'call'}, 3, 4));
function stringLength() {
  (arguments as any).length = '1';
  return sum.apply({tag: 'string'}, arguments as any);
}
console.log((stringLength as any)(10, 20));
function spreadStringLength() {
  (arguments as any).length = '1';
  return sum.call({tag: 'spread-string'}, ...arguments);
}
console.log((spreadStringLength as any)(10, 20));
function customIterator() {
  (arguments as any)[Symbol.iterator] = function* () { yield 21; yield 22; };
  return sum.call({tag: 'iter'}, ...arguments);
}
console.log((customIterator as any)(1, 2));
const nextOwner = Object.getPrototypeOf([][Symbol.iterator]());
const savedNext = nextOwner.next;
let nextStep = 0;
nextOwner.next = function() {
  return nextStep++ === 0 ? {value: 81, done: false} : {done: true};
};
const changedNext = (spread as any).call({tag: 'next'}, 1, 2);
nextOwner.next = savedNext;
console.log(changedNext);
let retained: any;
Object.defineProperty(sum, 'apply', {configurable: true, get() {
  console.log('lookup');
  return function(this: any, x: any, a: any) { retained = a; return this === sum; };
}});
console.log('own', (forward as any)(31, 32));
console.log('retained', retained[0], retained.length, Array.isArray(retained));
delete (sum as any).apply;
console.log('deleted-own-apply', (forward as any).call({tag: 'deleted'}, 33, 34));
Object.defineProperty(sum, 'call', {configurable: true, value: function(this: any, x: any, ...a: any[]) {
  return `${this === sum}:${a.join(',')}`;
}});
console.log('own-call', (spread as any)(41, 42));
delete (sum as any).call;
console.log('deleted-own-call', (spread as any).call({tag: 'deleted-call'}, 43, 44));
function throwingBody() { throw 'body'; }
function forwardThrow() { return throwingBody.apply(null, arguments); }
try { (forwardThrow as any)(1); } catch (e) { console.log('caught', e); }
function throwingIndex() {
  Object.defineProperty(arguments, '0', {get() { throw 'index'; }});
  return sum.apply(null, arguments as any);
}
try { (throwingIndex as any)(1); } catch (e) { console.log('caught', e); }
function throwingIterator() {
  (arguments as any)[Symbol.iterator] = function() { throw 'iterator'; };
  return sum.call(null, ...arguments);
}
try { (throwingIterator as any)(1); } catch (e) { console.log('caught', e); }
function disabledIterator() {
  (arguments as any)[Symbol.iterator] = undefined;
  return sum.call(null, ...arguments);
}
try { (disabledIterator as any)(1); } catch (e) { console.log('caught disabled-iterator'); }
const wideBody = function(...xs: any[]) { return `${xs.length}:${xs[64]}`; };
const wideForward = function() { return wideBody.apply(null, arguments as any); };
console.log('wide', (wideForward as any)(...Array.from({length: 65}, (_, i) => i)));
