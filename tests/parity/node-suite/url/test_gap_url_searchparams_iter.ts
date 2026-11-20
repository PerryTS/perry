// URLSearchParams iteration parity (mysql2 parseUrl pattern + dynamic surface).
// 1. for-of over a URL-adopted searchParams (default @@iterator → [key, value] pairs)
const u = new URL('mysql://user:pw@dbhost:3306/mydb?connectionLimit=5&x=1');
for (const [k, v] of u.searchParams) console.log('adopted', k, v);

// 2. for-of over an EMPTY adopted searchParams (mysql2 hits this for query-less URIs)
const bare = new URL('mysql://user:pw@dbhost:3306/mydb');
let empty = 0;
for (const _pair of bare.searchParams) empty++;
console.log('empty count', empty);

// 3. builtin held in a variable (minified-bundle shape)
const R = URL;
const u2 = new R('https://h/p?a=1&b=2');
for (const [k, v] of u2.searchParams) console.log('via-var', k, v);

// 4. free-standing URLSearchParams
const sp = new URLSearchParams('q=1&w=2&q=3');
for (const [k, v] of sp) console.log('free', k, v);

// 5. type-erased receiver: methods must dispatch dynamically
const erased: any = u2.searchParams;
console.log('get', erased.get('a'));
console.log('getAll', sp.getAll('q'));
for (const p of erased.entries()) console.log('entries', p[0], p[1]);
for (const k of erased.keys()) console.log('key', k);
for (const v of erased.values()) console.log('value', v);
erased.forEach((v: any, k: any) => console.log('fe', k, v));

// 6. has/delete with the Node 19+ two-arg forms through a dynamic receiver
const dsp: any = new URLSearchParams('m=1&m=2&n=3');
console.log('has2', dsp.has('m', '2'), dsp.has('m', '9'));
dsp.delete('m', '1');
console.log('after delete2', dsp.toString());

// 7. sort through a dynamic receiver
const ssp: any = new URLSearchParams('z=26&a=1&k=11');
ssp.sort();
console.log('sorted', ssp.toString());

// 8. spread still works (regression guard for the #1668 path)
console.log('spread', JSON.stringify([...sp]));
