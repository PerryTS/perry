// Class accessors are properties of the current holder, with the instance as this.
class Base {
  value = 4;
  get score(): number { return this.value + 1; }
  set score(v: number) { this.value = v - 1; }
}
class Mid extends Base {}
class Leaf extends Mid {}
class Tip extends Leaf {}
function read(x: any): number { return x.score; }
function write(x: any, v: number): void { x.score = v; }
const own: any = new Base();
const deep: any = new Tip();
console.log('declared', read(own), read(deep));
// Repeated stores must use the same inherited holder until it changes.
for (let i = 0; i < 100; i++) write(deep, i);
write(deep, 20);
console.log('subclass setter', read(deep), deep.value, Object.hasOwn(deep, 'score'));
let total = 0;
for (let i = 0; i < 100; i++) total += read(own) + read(deep);
console.log('warm', total);
Object.defineProperty(Base.prototype, 'score', {
  configurable: true,
  get() { return this.value + 100; },
  set(v: number) { this.value = v + 2; }
});
console.log('replaced', read(own), read(deep));
write(deep, 30);
console.log('replaced setter', read(deep), deep.value, Object.hasOwn(deep, 'score'));
// An intermediate holder now shadows the previously cached deep getter.
Object.defineProperty(Leaf.prototype, 'score', {
  configurable: true,
  get() { return this.value + 200; }
});
console.log('shadow', read(deep), read(own));
delete (Leaf.prototype as any).score;
console.log('unshadow', read(deep));
// A relink must stop the old declared accessor from answering.
const alternate = { get score() { return this.value + 300; } };
Object.setPrototypeOf(deep, alternate);
console.log('relinked', read(deep));
Object.defineProperty(alternate, 'score', { value: 7, writable: true, configurable: true });
// A holder with another accessor still has to respect this data shadow.
Object.defineProperty(alternate, 'other', { get() { return 99; } });
console.log('data replacement', read(deep));
write(deep, 8);
console.log('own data', read(deep), Object.hasOwn(deep, 'score'));
// An inherited setter also intercepts a key never declared by the class.
Object.defineProperty(Base.prototype, 'late', {
  configurable: true,
  set(v: number) { this.value = v * 2; },
  get() { return this.value; }
});
const late: any = new Tip();
late.late = 9;
console.log('late', late.late, late.value, Object.hasOwn(late, 'late'));
// ClassBody methods define own properties even above an inherited setter.
class DefinitionParent {
  set method(v: any) { throw new Error('definition ran inherited setter'); }
}
class DefinitionChild extends DefinitionParent {
  method() { return 42; }
  get marker() { return 1; }
}
const defined: any = new DefinitionChild();
console.log('own definition', defined.method(), defined.marker);
