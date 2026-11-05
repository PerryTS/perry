// Spec Get observability across ordinary data reads and fallback receivers.
function check(actual: any, expected: any, label: string) {
  if (!Object.is(actual, expected)) throw new Error(label);
  console.log(label, true);
}

const proto: any = { value: 7, absent: 9 };
const object: any = Object.create(proto);
check(Reflect.get(object, "value"), 7, "inherited data");
object.value = 11;
check(Reflect.get(object, "value"), 11, "shadowed data");
delete object.value;
check(Reflect.get(object, "value"), 7, "deleted shadow");
object.absent = undefined;
check(Reflect.get(object, "absent"), undefined, "own undefined");
Object.freeze(proto);
check(Reflect.get(object, "value"), 7, "frozen prototype");
Object.freeze(object);
check(Reflect.get(object, "absent"), undefined, "frozen own data");

let calls = 0;
const owner: any = { value: 1, marker: 13 };
Object.defineProperty(owner, "value", {
  configurable: true,
  get() { calls++; return this.marker; },
});
check(Reflect.get(owner, "value"), 13, "own getter result");
check(calls, 1, "own getter once");
check(Reflect.get(owner, "value", { marker: 17 }), 17, "distinct receiver");
check(calls, 2, "distinct getter once");
const child: any = Object.create(owner);
child.marker = 19;
check(Reflect.get(child, "value"), 19, "inherited getter receiver");
check(calls, 3, "inherited getter once");
Object.defineProperty(child, "value", { value: 23 });
check(Reflect.get(child, "value"), 23, "data shadows getter");
check(calls, 3, "shadow suppresses getter");
Object.defineProperty(owner, "setterOnly", { set(value) {} });
check(Reflect.get(owner, "setterOnly"), undefined, "setter only");

let traps = 0;
const proxy: any = new Proxy({ value: 29 }, {
  get(target, key, receiver) {
    traps++;
    return Reflect.get(target, key, receiver);
  },
});
check(Reflect.get(proxy, "value"), 29, "proxy result");
check(traps, 1, "proxy once");
check(Reflect.get(Object.create(proxy), "value"), 29, "proxy prototype");
check(traps, 2, "prototype proxy once");
const revoked = Proxy.revocable({}, {});
revoked.revoke();
let threw = false;
try { Reflect.get(revoked.proxy, "value"); } catch { threw = true; }
check(threw, true, "revoked proxy throws");

let order = "";
const key: any = { [Symbol.toPrimitive]() { order += "key;"; return "value"; } };
const observed: any = { get value() { order += "get;"; return 31; } };
check(Reflect.get(observed, key), 31, "coerced key result");
check(order, "key;get;", "coercion then getter once");
const symbol = Symbol("value");
check(Reflect.get({ [symbol]: 37 }, symbol), 37, "symbol key");
check(Reflect.get({ "#value": 41 }, "#value"), 41, "private-looking string");
const longKey = "a property name longer than the inline owned string copy capacity, for borrowed key coverage";
check(Reflect.get({ [longKey]: 43 }, longKey), 43, "long key");

class Base {
  static value = 47;
  #value = 53;
  read() { return this.#value; }
}
class Derived extends Base {}
check(Reflect.get(Derived, "value"), 47, "inherited class static");
check(new Derived().read(), 53, "private field");
const expression: any = class { static value = 59; };
check(Reflect.get(expression, "value"), 59, "class expression static");
const fn: any = () => 0;
fn.value = 61;
check(Reflect.get(fn, "value"), 61, "closure expando");
const array: any = [1, 2];
array.value = 67;
check(Reflect.get(array, "value"), 67, "array expando");
check(Reflect.get(array, "length"), 2, "array length");
check(Reflect.get(new Uint8Array([71]), "0"), 71, "typed array index");
check(Reflect.get(new Date(0), "constructor"), Date, "date constructor");
const re: any = /x/g;
re.lastIndex = 3;
re.value = 73;
check(Reflect.get(re, "value"), 73, "regexp expando");
check(Reflect.get(re, "lastIndex"), 3, "regexp lastIndex");
Object.defineProperty(re, "value", { get() { calls++; return 79; } });
check(Reflect.get(re, "value"), 79, "regexp accessor");
check(calls, 4, "regexp getter once");
let jsonGets = 0;
let jsonCalls = 0;
const jsonPrototype = {
  get toJSON() {
    jsonGets++;
    return function () { jsonCalls++; return this.answer; };
  }
};
const jsonChild: any = Object.create(jsonPrototype);
jsonChild.answer = 83;
check(JSON.stringify(jsonChild), "83", "inherited toJSON getter receiver");
check(jsonGets, 1, "toJSON getter once");
check(jsonCalls, 1, "toJSON method once");
check(JSON.stringify({ toJSON: 0, value: 89 }), '{"toJSON":0,"value":89}', "noncallable toJSON");
console.log("native property Get parity passed");
