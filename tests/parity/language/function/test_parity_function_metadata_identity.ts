import { inspect, deprecate, types } from 'node:util';
function named(a, b) { return a + b; }
const alias = named;
console.log('alias', alias === named, alias.name, alias.length, alias(2, 3));
function make() { return function same() { return 7; }; }
const first = make(), second = make();
Object.defineProperty(first, 'name', { value: 'ownFirst', configurable: true });
Object.defineProperty(second, 'name', { value: 'ownSecond', configurable: true });
console.log('own', first.name, second.name, first(), second(), first !== second);
console.log('source', first.toString() === second.toString(), named.toString());
const ordinary = function ordinary() { return 1; };
const arrow = () => 2;
const bound = named.bind(null, 1);
console.log('kinds', ordinary.name, arrow.name, bound.name, bound.length, bound(4));
const sym = Symbol('metadata');
const object = { [sym]: function () { return 3; }, get x() { return 4; }, m() { return 5; } };
const getter = Object.getOwnPropertyDescriptor(object, 'x')!.get!;
console.log('inferred', object[sym].name, getter.name, object.m.name, object[sym](), getter.call(object), object.m());
console.log('method-source', object.m.toString());
class Example {
  m() { return 6; }
  static n() { return 8; }
  get x() { return 9; }
}
const instance = new Example();
const classGetter = Object.getOwnPropertyDescriptor(Example.prototype, 'x')!.get!;
console.log('class', instance.m.name, Example.n.name, classGetter.name, instance.m(), Example.n(), classGetter.call(instance));
console.log('class-source', instance.m.toString(), Example.n.toString(), classGetter.toString());
console.log('native', Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength')!.get!.name);
const deprecated = deprecate(named, 'metadata witness');
console.log('deprecated', deprecated.name);
async function asyncBody() { return 1; }
function* generatorBody() { yield 1; }
console.log('flags', types.isAsyncFunction(asyncBody), types.isGeneratorFunction(generatorBody), types.isAsyncFunction(named));
console.log('inspect', inspect(named), inspect(alias));

class Odd { static ['\ud800']() { return 11; } }
console.log('wtf8', Odd['\ud800'].name.charCodeAt(0), Odd['\ud800']());
