// Skipping primitive toJSON bookkeeping must retain callback order and keys.
function identity(key: string, value: any): any { return value; }
const calls: string[] = [];
function makeHook(label: string): any {
    return { toJSON(key: string) { calls.push(label + ':' + key); return label; } };
}
const value: any = { scalar: 1, text: 'plain', first: makeHook('first'), flag: true, nested: { before: null, after: makeHook('nested') }, list: [1, 's', makeHook('array')] };
console.log(JSON.stringify(value, identity));
console.log(JSON.stringify(calls));
const callbackKeys: string[] = [];
console.log(JSON.stringify(value, function(key: string, v: any): any {
    callbackKeys.push(key);
    if (key === 'text') {
        const inner = { before: 1, inside: makeHook('reentrant') };
        console.log('inner', JSON.stringify(inner, identity));
    }
    return v;
}, 2));
console.log(JSON.stringify(callbackKeys));
console.log(JSON.stringify(calls));
(BigInt.prototype as any).toJSON = function(key: string): string { return 'big:' + key; };
console.log(JSON.stringify({ before: 3, big: 42n, after: makeHook('last') }, identity));
delete (BigInt.prototype as any).toJSON;
console.log(JSON.stringify({ big: 42n }, function(key: string, v: any): any { return typeof v === 'bigint' ? String(v) : v; }));
