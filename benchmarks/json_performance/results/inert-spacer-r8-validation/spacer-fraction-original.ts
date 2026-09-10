const text = '{"id":7,"name":"Ada","active":true,"tags":[1,2]}';
const value: any = JSON.parse(text);
const spacers: any[] = [undefined, null, false, true, 0, -0, 0.5, -2, NaN, '', 1, 2, '..'];
for (let i = 0; i < spacers.length; i++) {
    console.log('space', i, JSON.stringify(value, null, spacers[i]));
    console.log('keys', i, JSON.stringify(value, ['name', 'id'], spacers[i]));
}
console.log('literal-zero', JSON.stringify(value, null, 0));
console.log('literal-negative-zero', JSON.stringify(value, null, -0));
console.log('literal-true', JSON.stringify(value, null, true));
let events: string[] = [];
const custom: any = {toJSON(key: string) {events.push('toJSON:' + key); return {answer: 42};}};
for (const spacer of [0, -0, true]) {
    events = [];
    console.log('callback', JSON.stringify({item: custom}, function(key: string, v: any) {
        events.push('replace:' + key);
        return v;
    }, spacer));
    console.log('events', events.join('|'));
}
let coercions = 0;
const boxed: any = new Number(0);
boxed.valueOf = function() {coercions++; return 0;};
console.log('boxed-zero', JSON.stringify(value, null, boxed), coercions);
boxed.valueOf = function() {coercions++; return 2;};
console.log('boxed-two', JSON.stringify(value, null, boxed), coercions);
boxed.valueOf = function() {throw new Error('space-threw');};
try {JSON.stringify(value, null, boxed);} catch (e: any) {console.log('caught', e.message);}
console.log('after-throw', JSON.stringify(value, null, 0));
const boxedString: any = new String('');
boxedString.toString = function() {coercions++; return '..';};
console.log('boxed-string', JSON.stringify({a: 1}, null, boxedString), coercions);
// Mutating toJSON invalidates output reuse even when spacing remains inert.
const changed: any = {a: 1};
console.log('before-mutation', JSON.stringify(changed, null, 0));
changed.toJSON = function() {return {b: 2};};
console.log('after-mutation', JSON.stringify(changed, null, 0));
let checksum = 0;
const saved: any[] = [];
for (let i = 0; i < 240; i++) {
    const parsed: any = JSON.parse(text);
    const output = JSON.stringify(parsed, null, spacers[i % 6]);
    checksum += output.length;
    if (i % 31 === 0) saved.push({input: parsed, output: output});
    const pressure: any[] = [];
    for (let j = 0; j < 40; j++) pressure.push({id: j, text: 'keep-' + j});
    checksum += pressure[39].id;
}
console.log('retained', checksum, saved.length, saved[0].output, saved[7].input.name);
