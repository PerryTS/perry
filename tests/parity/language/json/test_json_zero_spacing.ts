// Numeric zero uses compact output while callbacks and spacer coercion remain observable.
function show(label: string, value: any, spacer: any, replacer: any = null): void {
    console.log(label, JSON.stringify(value, replacer, spacer));
}

const zeros: any[] = [0, -0, Number('0')];
const values: any[] = [
    null, true, false, undefined, 0, -0, 1.25, '', 'a', '東京🙂',
    'escaped\n"quote"\\tail', {}, {a: 1}, {a: 'text', b: true},
    {id: 42, name: 'user_42', active: false, tags: ['a', 'b']},
    {'2': 'two', '1': 'one', a: 'last'}, [1, true, null, 'tail'],
];
for (let z = 0; z < zeros.length; z++) {
    for (let i = 0; i < values.length; i++) show('value-' + z + '-' + i, values[i], zeros[z]);
}

let calls = 0;
const getter: any = {a: 1};
Object.defineProperty(getter, 'b', {
    enumerable: true,
    get: function(): any {
        calls++;
        const keep: any[] = [];
        for (let i = 0; i < 48; i++) keep.push({id: i, text: 'getter-' + i});
        return keep[47].id;
    },
});
show('getter-first', getter, 0);
show('getter-second', getter, -0);
console.log('getter-calls', calls);

const own: any = {id: 7};
own.toJSON = function(key: string): any {
    const keep: any[] = [];
    for (let i = 0; i < 48; i++) keep.push({id: i, text: 'tojson-' + i});
    return {key: key, id: keep[47].id};
};
show('own-toJSON', own, 0);
show('nested-toJSON', {child: own}, -0);

const proto: any = {
    toJSON: function(key: string): any { return {inherited: key}; },
};
const inherited: any = Object.create(proto);
inherited.id = 3;
show('inherited-toJSON', inherited, 0);

const mutable: any = {id: 1, name: 'before'};
for (let i = 0; i < 4; i++) show('warm-' + i, mutable, 0);
mutable.name = 'after\nchange';
show('mutated', mutable, -0);
Object.defineProperty(mutable, 'id', {
    enumerable: true,
    get: function(): number { return 99; },
});
show('descriptor-after-cache', mutable, 0);

let spacerCalls = 0;
const spacerTarget: any = {id: 1};
const boxedZero: any = new Number(0);
boxedZero.valueOf = function(): number {
    spacerCalls++;
    spacerTarget.id = 9;
    return 0;
};
show('boxed-spacer', spacerTarget, boxedZero);
console.log('spacer-calls', spacerCalls);
show('pretty-number', {a: 1}, 2);
show('string-zero-is-indent', {a: 1}, '0');
show('keys', {a: 1, b: 2, nested: {a: 3, b: 4}}, 0, ['b', 'nested']);

let callbackCalls = 0;
show('callback', {id: 7, name: 'keep'}, 0, function(key: string, value: any): any {
    callbackCalls++;
    const keep: any[] = [];
    for (let i = 0; i < 48; i++) keep.push({id: i, text: 'callback-' + i});
    if (key === 'id') return value + keep[47].id;
    return value;
});
console.log('callback-calls', callbackCalls);

// Keep inputs and outputs live across allocating loop polls. Scheduled runs
// must report positive copied-object and protected-from-space counters.
const retained: string[] = [];
let checksum = 0;
for (let round = 0; round < 80; round++) {
    const record: any = JSON.parse('{"id":' + round + ',"name":"retained record ' + round + '","tags":["a","b"]}');
    for (let repeat = 0; repeat < 8; repeat++) {
        const result: string = JSON.stringify(record, null, zeros[repeat % 3]);
        retained.push(result);
        checksum += result.length;
        const churn: any = JSON.parse('{"id":' + repeat + ',"name":"allocation between calls"}');
        if (churn.id !== repeat) throw new Error('churn changed');
    }
}
for (let i = 0; i < retained.length; i++) {
    const record: any = JSON.parse(retained[i]);
    if (record.id !== Math.floor(i / 8) || record.tags[1] !== 'b') throw new Error('retained output changed');
    checksum += record.id;
}
console.log('retained', retained.length, checksum);
