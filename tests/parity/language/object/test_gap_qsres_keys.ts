// Fresh enumeration shares key words, never the receiver's mutable result.
function show(label: string, value: any) {
    console.log(label + ':' + Object.keys(value).join('|'));
}
const small: any = { qsres_long_first: 1, qsres_long_second: 2, qsres_long_third: 3 };
show('small', small);
const first = Object.keys(small);
first[0] += '_changed';
first.push('extra');
show('fresh', small);
console.log('result:' + first.join('|'));
const ordered: any = { z: 1, '10': 2, '2': 3, '01': 4, '4294967295': 5, '4294967294': 6, a: 7 };
show('indices', ordered);
delete ordered['2'];
delete ordered.z;
ordered.z = 8;
show('deleted-reinserted', ordered);
let gets = 0;
const attrs: any = { a: 1 };
Object.defineProperty(attrs, 'hidden', { value: 2, enumerable: false, configurable: true });
Object.defineProperty(attrs, 'getter', { get() { gets++; return 3; }, enumerable: true, configurable: true });
attrs.b = 4;
show('attrs', attrs);
console.log('getters:' + gets);
delete attrs.getter;
show('deleted-accessor', attrs);
const symbol = Symbol('qsres');
small[symbol] = 5;
show('symbol-filter', small);
const empty: any = {};
show('empty', empty);
show('null-prototype', Object.assign(Object.create(null), { b: 1, '1': 2, a: 3 }));
const array: any = [1, , 3];
array.named = 4;
show('array', array);
show('string', 'abc');
show('number', 42);
const proxy: any = new Proxy({ a: 1, b: 2 }, {
    ownKeys() { return ['b', 'a']; }
});
show('proxy', proxy);
