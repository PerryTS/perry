// Small native method calls share the holder proof; overrides and effectful
// arguments still perform one Get before evaluating the arguments.
function replace(s: any, pattern: any, value: any) { return s.replace(pattern, value); }
function split(s: any, separator: any, limit: any) { return s.split(separator, limit); }
function index(s: any, needle: any, position: any) { return s.indexOf(needle, position); }
function slice(a: any, start: any, end: any) { return a.slice(start, end); }
for (let i = 0; i < 20; i++) {
    replace('axbxc', /x/g, 'y');
    split('a&b&c', '&', undefined);
    index('axbxc', 'b', 0);
    slice([1, 2, 3], undefined, undefined);
}
console.log(replace('axbxc', /x/g, 'y'));
console.log(replace('ab', 'b', undefined));
console.log(split('a&b&c', '&', 2).join('|'));
console.log(index('axbxc', 'b', 0), index('abc', undefined, 0));
console.log(index('é🙂abc', 'a', 0), index('abc', '', Infinity));
console.log(slice([1, 2, 3], undefined, undefined).join('|'));
let order = '';
console.log(String.prototype.indexOf.call(
    { toString() { order += 'r'; return 'axb'; } },
    { toString() { order += 's'; return 'b'; } },
    { valueOf() { order += 'p'; return 0; } },
));
console.log(order, String.prototype.indexOf.length);
try { String.prototype.indexOf.call(null, 'x'); } catch (e) { console.log(e instanceof TypeError); }
class Custom {
    replace(a: any, b: any) { return 'custom:' + a + b; }
    split(a: any, b: any) { return ['custom', a, b]; }
    indexOf(a: any, b: any) { return 77; }
    slice(a: any, b: any) { return ['custom']; }
}
const custom: any = new Custom();
console.log(replace(custom, 1, 2));
console.log(split(custom, 1, 2).join('|'));
console.log(index(custom, 1, 2));
console.log(slice(custom, 1, 2).join('|'));
const saved = String.prototype.indexOf;
function effectful(s: any, change: any) { return s.indexOf(change(), 0); }
console.log(effectful('abc', () => { String.prototype.indexOf = () => 99; return 'b'; }));
String.prototype.indexOf = saved;
console.log(index('abc', 'b', 0));
