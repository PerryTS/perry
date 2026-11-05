// Warmed DefineField edges must retain definition semantics and initializer order.
let setters = 0;
class Base {
  set x(value: number) { setters++; }
  a = 10;
}
const computed = 'computed';
class Child extends Base {
  x = 20;
  b = this.a + this.x;
  [computed] = 30;
}
for (let i = 0; i < 3; i++) {
  const value: any = new Child();
  console.log(value.a, value.x, value.computed, value.b, setters, Object.keys(value).join(','));
}
let escaped: any;
let fail = false;
function save(value: any) { escaped = value; return 1; }
function maybeThrow() { if (fail) throw new Error('initializer'); return 2; }
class Partial {
  first = save(this);
  second = maybeThrow();
  third = 3;
}
new Partial();
fail = true;
try { new Partial(); } catch (e) { console.log('throws', Object.keys(escaped).join(','), Object.hasOwn(escaped, 'second'), Object.hasOwn(escaped, 'third')); }
fail = false;
console.log(Object.keys(new Partial()).join(','));
// A base constructor may replace this with an accessor-bearing object.
let accessorCalls = 0;
class Replacement {
  constructor() {
    const object: any = {};
    Object.defineProperty(object, 'x', { set(v) { accessorCalls++; }, configurable: true });
    return object;
  }
}
class DefineAccessor extends Replacement { x = 7; }
for (let i = 0; i < 3; i++) {
  const object: any = new DefineAccessor();
  console.log('accessor', object.x, accessorCalls, Object.getOwnPropertyDescriptor(object, 'x')!.writable);
}
class Closed {
  constructor() { return Object.preventExtensions({}); }
}
class DefineClosed extends Closed { x = 7; }
try { new DefineClosed(); } catch (e) { console.log('closed', e instanceof TypeError); }
let defines = 0, sets = 0;
class Proxied {
  constructor() { return new Proxy({}, {
    defineProperty(target, key, descriptor) { defines++; return Reflect.defineProperty(target, key, descriptor); },
    set(target, key, value) { sets++; return Reflect.set(target, key, value); }
  }); }
}
class DefineProxy extends Proxied { x = 8; y = 9; }
for (let i = 0; i < 3; i++) {
  const value: any = new DefineProxy();
  console.log('proxy', value.x, value.y, defines, sets);
}
const symbol = Symbol('field');
class SymbolField { ordinary = 5; [symbol] = 44; }
for (let i = 0; i < 3; i++) {
  const value: any = new SymbolField();
  console.log('symbol', value[symbol], value.ordinary, Object.getOwnPropertySymbols(value).length);
}
