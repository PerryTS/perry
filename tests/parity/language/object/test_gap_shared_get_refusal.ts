// Generic named reads must preserve the complete read's receiver and effects.
let gets = 0;
function readValue(value: any): any { return value.refusalValue; }
function readLength(value: any): any { return value.length; }
function printLength(name: string, value: any): void {
  try { console.log(name, readLength(value)); }
  catch (error) { console.log(name, error instanceof TypeError); }
}
const proto: any = {};
Object.defineProperty(proto, 'refusalValue', {
  get() { gets++; return this.own + 3; }, configurable: true
});
const child: any = Object.create(proto);
child.own = 39;
console.log('missing', readValue({}));
console.log('getter', readValue(child), gets);
const proxy: any = new Proxy({ own: 40 }, {
  get(target, key, receiver) {
    if (key === 'refusalValue') { gets++; return receiver.own + 2; }
    if (key === 'length') { gets++; return 19; }
    return Reflect.get(target, key, receiver);
  }
});
console.log('proxy', readValue(proxy), gets);
console.log('strings', readValue('abc'), readValue('a long heap string'));
console.log('numbers', readValue(4), readValue(4.5));
console.log('boolean', readValue(true));
for (const value of [null, undefined]) {
  try { readValue(value); } catch (error) { console.log('nullish', error instanceof TypeError); }
}
printLength('array', [1, 2, 3]);
class ArrayChild extends Array {}
printLength('array-subclass', new ArrayChild(4));
printLength('short-string', 'abc');
printLength('heap-string', 'a longer heap string');
printLength('number', 4);
printLength('double', 4.5);
printLength('boolean', true);
printLength('null', null);
printLength('undefined', undefined);
printLength('object', { length: 7 });
const lengthProto: any = {};
Object.defineProperty(lengthProto, 'length', { get() { gets++; return this.own; } });
const lengthChild: any = Object.create(lengthProto);
lengthChild.own = 11;
printLength('getter-length', lengthChild);
printLength('proxy-length', proxy);
printLength('function', function(a: any, b: any) {});
printLength('class-reference', ArrayChild);
printLength('typed-array', new Uint8Array(5));
printLength('bigint', 7n);
printLength('symbol', Symbol('length'));
printLength('date', new Date(0));
printLength('regexp', /abc/);
printLength('global-object', globalThis);
printLength('namespace', Math);
printLength('console', console);
printLength('map', new Map());
printLength('set', new Set());
printLength('string-wrapper', new String('wrapper'));
printLength('number-wrapper', new Number(7));
console.log('gets', gets);
// Bare identifiers use the global binding entry, including its absence check.
(globalThis as any).BSZ2bBinding = 73;
console.log('global', BSZ2bBinding);
delete (globalThis as any).BSZ2bBinding;
try { console.log(BSZ2bBinding); }
catch (error) { console.log('absent-global', error instanceof ReferenceError); }
Object.defineProperty(globalThis, 'BSZ2bBinding', { get() { gets++; return 74; }, configurable: true });
console.log('global-getter', BSZ2bBinding, gets);
delete (globalThis as any).BSZ2bBinding;
