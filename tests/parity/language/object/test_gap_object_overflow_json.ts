const o: any = JSON.parse('{"z":1,"10":2,"2":3,"a":{"foo":4},"z":5}');
console.log(Object.keys(o).join(','), JSON.stringify(o));
o.foo = 'x';
delete o.z; o.z = 8;
console.log(JSON.stringify(o));
const copy = JSON.parse(JSON.stringify(o));
console.log(Object.keys(copy).join(','), copy.a.foo, copy.z);
Object.defineProperty(copy, 'quiet', { value: 12, enumerable: false });
console.log(Object.keys(copy).join(','), Object.getOwnPropertyNames(copy).join(','));
