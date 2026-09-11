// Exercise bounded output and the full fallback through the same public entry.
const record: any = JSON.parse('{"id":7,"name":"Ada","active":true,"tags":[1,2]}');
for (let i = 0; i < 3; i++) console.log('plain', JSON.stringify(record));
console.log('zero', JSON.stringify(record, null, 0));
console.log('pretty', JSON.stringify(record, null, 2));
console.log('keys', JSON.stringify(record, ['name', 'id', 'name']));
console.log('string-space', JSON.stringify({a: [1, 2]}, null, '..'));

let events: string[] = [];
const custom: any = {toJSON(key: string) {
    events.push('toJSON:' + key);
    return {value: 9};
}};
console.log('order', JSON.stringify({item: custom}, function(key: string, value: any) {
    events.push('replace:' + key);
    return value;
}, 1));
console.log('events', events.join('|'));
console.log('reentrant', JSON.stringify({a: 1, b: 2}, function(key: string, value: any) {
    if (key === 'a') return JSON.stringify({inside: value});
    return value;
}));
console.log('root-drop', JSON.stringify(record, function(key: string, value: any) {
    return key === '' ? undefined : value;
}));

try { JSON.stringify({toJSON() {throw new Error('toJSON-threw');}}); }
catch (e: any) { console.log('caught', e.message); }
console.log('after-toJSON', JSON.stringify(record));
try { JSON.stringify(record, function(key: string, value: any) {
    if (key === 'name') throw new Error('replacer-threw');
    return value;
}); }
catch (e: any) { console.log('caught', e.message); }
console.log('after-replacer', JSON.stringify(record));
const cyclic: any = {id: 1}; cyclic.self = cyclic;
try { JSON.stringify(cyclic); }
catch (e: any) { console.log('cycle', e instanceof TypeError); }
console.log('after-cycle', JSON.stringify(record));

let spacerCalls = 0;
const spacer: any = new Number(2);
spacer.valueOf = function() {spacerCalls++; return 1;};
console.log('boxed-space', JSON.stringify({a: 1}, null, spacer), spacerCalls);

// Allocate inside a callback while the full serializer's runtime frame is live.
let callbackChecksum = 0;
console.log('moving-callback', JSON.stringify({item: {id: 7}}, function(key: string, value: any) {
    const keep: any[] = [];
    for (let j = 0; j < 64; j++) {
        keep.push({id: j, text: 'callback-' + j});
        callbackChecksum += keep[j].id;
    }
    if (key === 'id') return value + 1;
    return value;
}), callbackChecksum);

// Keep both the original input and generated outputs live across copying GC.
const saved: any[] = [];
let checksum = 0;
for (let i = 0; i < 240; i++) {
    const value: any = JSON.parse('{"id":7,"name":"Ada","active":true,"tags":[1,2]}');
    const plain = JSON.stringify(value);
    const pretty = JSON.stringify(value, null, 1);
    checksum += plain.length + pretty.length;
    if (i % 31 === 0) saved.push({value: value, plain: plain, pretty: pretty});
    const pressure: any[] = [];
    for (let j = 0; j < 40; j++) pressure.push({id: j, text: 'keep-' + j});
    checksum += pressure[39].id;
}
console.log('pressure', checksum, saved.length, saved[0].plain, saved[7].value.name);
console.log('final', JSON.stringify(record));
