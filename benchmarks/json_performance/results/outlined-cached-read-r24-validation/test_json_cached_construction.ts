// Complete cached arrays without duplicating records, preserving aliases and
// mutations across construction, moving collections and later stringify.
declare function gc(): void;

function record(index: number): string {
    const id = '"id":' + index;
    if (index % 7 === 1) {
        return '{' + id + ',"name":"\\u0061\\ud800\\n","tags":[],"n":-0}';
    }
    if (index % 7 === 2) {
        return '{' + id + ',"\\u0069d":' + (index + 10) + ',"name":"é😀","tags":[null,true,0.1]}';
    }
    if (index % 7 === 3) {
        return '{' + id + ',"nested":{"x":1,"x":2},"tags":[[1],[2]]}';
    }
    if (index % 7 === 4) {
        return '{' + id + ',"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8}';
    }
    if (index % 7 === 5) {
        return '{' + id + ',"__proto__":{"safe":true},"tags":[],"n":1e300}';
    }
    return '{' + id + ',"name":"record_' + index + '","tags":["red","blue"],"active":true,"n":0.10000000000000001}';
}

const parts: string[] = [];
for (let i = 0; i < 240; i++) parts.push(record(i));
const text = '[ ' + parts.join(' , ') + ' ]';
const retained: any[] = [];
let checksum = 0;
for (let round = 0; round < 24; round++) {
    const parsed = JSON.parse(text);
    const first = parsed[0];
    first.name = 'changed_' + round;
    first.tags.push('extra');
    for (let i = 0; i < parsed.length; i++) checksum += parsed[i].id;
    if (parsed[0] !== first) throw new Error('lost cached identity');
    if (parsed[0].name !== 'changed_' + round) throw new Error('lost cached mutation');
    if (parsed[0].tags.length !== 3) throw new Error('lost nested mutation');
    if (round % 6 === 0) retained.push(parsed);
    if (round % 4 === 0) gc();
    checksum += parsed[1].name.length + parsed[2].tags.length;
}
gc();
console.log('checksum', checksum);
for (const parsed of retained) {
    console.log(JSON.stringify(parsed));
}

// A stringify-triggered force must preserve the same sparse alias, too.
const stringifyFirst = JSON.parse(text);
const alias = stringifyFirst[0];
alias.name = 'stringify_first';
console.log(JSON.stringify(stringifyFirst));
gc();
console.log('alias', stringifyFirst[0] === alias, alias.name);

// More than 2048 slots puts the completed root array on the large allocation
// path. Cached young objects must remain traceable from that completed array.
const largeParts: string[] = [];
for (let i = 0; i < 2600; i++) largeParts.push(record(i));
const largeText = '[' + largeParts.join(',') + ']';
for (let round = 0; round < 4; round++) {
    const parsed = JSON.parse(largeText);
    const aliases: any[] = [];
    for (let i = 0; i < 32; i++) {
        aliases.push(parsed[i]);
        parsed[i].changed = round * 100 + i;
    }
    gc();
    parsed.push({ id: -1, tags: ['tail'] });
    for (let i = 0; i < 32; i++) {
        if (parsed[i] !== aliases[i]) throw new Error('lost large cached alias');
        if (parsed[i].changed !== round * 100 + i) throw new Error('lost large mutation');
        aliases[i].later = { value: i + round };
    }
    gc();
    let total = 0;
    for (let i = 0; i < parsed.length; i++) total += parsed[i].id;
    console.log('large', round, parsed.length, total);
    console.log(JSON.stringify(parsed));
}
