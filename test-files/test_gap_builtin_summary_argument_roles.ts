// Callback argument roles, ignored arguments, conversion hooks and overrides.
class Item { value: number; constructor(value: number) { this.value = value; } }
class Context { scale: number; constructor(scale: number) { this.scale = scale; } }
const items: any = [new Item(2), new Item(3)];
const context = new Context(4);
const mapped = items.map(function(this: any, item: any, index: number, array: any[]) {
  return new Item(item.value * this.scale + index + array.length);
}, context);
console.log(mapped[0].value, mapped[1].value);
console.log(items.reduce((acc: any, item: any) => new Item(acc.value + item.value), new Item(1)).value);
console.log(items.sort((a: any, b: any) => b.value - a.value)[0].value);
console.log(Array.from(items, function(this: any, item: any) { return item.value + this.scale; }, context).join(','));
let calls = 0;
function ignored(item: any) { calls++; return item.value; }
const text: any = 'abc';
text.trim(ignored); text.charAt(0, ignored); text.slice(0, 2, ignored);
console.log(calls, Number.isNaN(ignored), Boolean(ignored));
const index = { valueOf() { calls++; return 1; } };
console.log(text.slice(index), calls);
console.log(text.replace('b', function(match: string, offset: number, input: string) { return match + offset + input.length; }));
const parsed = JSON.parse('{"a":2}', function(key: string, value: any) { return key === 'a' ? new Item(value) : value; });
console.log(parsed.a.value);
console.log(JSON.stringify({a: new Item(5)}, function(key: string, value: any) { return key === 'value' ? value + 1 : value; }));
const original = (String.prototype as any).forEach;
(String.prototype as any).forEach = function(cb: any) { return cb(new Item(7)); };
console.log(text.forEach((item: any) => item.value));
if (original === undefined) delete (String.prototype as any).forEach;
else (String.prototype as any).forEach = original;
