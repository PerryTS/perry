const namespace: any = JSON;
const key = process.argv[2] || 'parse';
const parse = namespace[key];
const value = parse('{"answer":42,"items":[1,2,3]}');
console.log(value.answer, value.items.length, value.items[2]);
const host: any = globalThis;
const name = process.argv[3] || 'JSON';
console.log(host[name][key]('[4,5]')[1]);
