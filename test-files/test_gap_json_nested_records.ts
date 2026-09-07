// Mixed schemas, native-only traversal, and observable fallback side effects.
function makeRows(count: number): any[] {
    let text = '{"data":[';
    for (let i = 0; i < count; i++) {
        if (i > 0) text += ',';
        text += '{"id":' + i + ',"field_' + (i % 32) + '":' + (i * 2)
            + ',"nested":{"value":"heap value ' + i + '","yes":true},"tail":"original"}';
    }
    return JSON.parse(text + ']}').data;
}
function hash(text: string): number {
    let result = 0;
    for (let i = 0; i < text.length; i++) result = (result * 31 + text.charCodeAt(i)) % 1000000007;
    return result;
}

const ordinary = makeRows(96);
const encoded = JSON.stringify(ordinary);
console.log('ordinary', encoded.length, hash(encoded));
ordinary[95].id = -0;
ordinary[95].nested.value = '東京🙂\n"quote"';
console.log('changed', hash(JSON.stringify(ordinary)));

for (const position of [0, 31, 63]) {
    const input = makeRows(64);
    let calls = 0;
    Object.defineProperty(input[position].nested, 'value', {
        enumerable: true,
        get() {
            calls++;
            input[63].tail = 'changed by getter';
            let scratch: any[] = [];
            for (let j = 0; j < 80; j++) scratch.push(JSON.parse('{"allocate":"heap child in callback"}'));
            return 'getter:' + position + ':' + scratch.length;
        }
    });
    const result = JSON.stringify(input);
    console.log('getter', position, calls, result.length, hash(result));
}

for (const position of [0, 31, 63]) {
    const input = makeRows(64);
    let calls = 0;
    input[position].nested.toJSON = function(key: string) {
        calls++;
        input[63].tail = 'changed by toJSON';
        const reentrant = JSON.stringify(makeRows(3));
        return { key, nestedLength: reentrant.length, calls };
    };
    console.log('toJSON', position, hash(JSON.stringify(input)), calls);
}

const modified = makeRows(3);
modified[1].nested = { '20': 20, '3': 3, z: null };
modified[2].id = undefined;
modified[2].tail = function() { return 1; };
console.log('ordering-omissions', JSON.stringify(modified));
delete modified[0].tail;
Object.defineProperty(modified[1], 'tail', { enumerable: false });
modified[2].id = Symbol('omitted');
console.log('deleted-hidden', JSON.stringify(modified));
Object.setPrototypeOf(modified[0].nested, { toJSON(key: string) { return 'inherited:' + key; } });
console.log('prototype', JSON.stringify(modified));

const sparse = makeRows(3);
delete sparse[1];
console.log('sparse', JSON.stringify(sparse));
const cyclic = makeRows(3);
cyclic[2].nested.back = cyclic;
let caught = false;
try { JSON.stringify(cyclic); } catch (_) { caught = true; }
console.log('cycle', caught);
const throwing = makeRows(3);
let throws = 0;
Object.defineProperty(throwing[2].nested, 'value', {
    enumerable: true,
    get() { throws++; throw new Error('expected'); }
});
caught = false;
try { JSON.stringify(throwing); } catch (_) { caught = true; }
console.log('throw', caught, throws);
console.log('after-throw', hash(JSON.stringify(makeRows(64))));

// Retained outputs are distinct across calls and collections.
const retained: string[] = [];
let checksum = 0;
for (let i = 0; i < 1600; i++) {
    const input = makeRows(4);
    input[3].id = i;
    const output = JSON.stringify(input);
    retained.push(output);
    checksum += output.length;
}
console.log('retained', retained.length, checksum, hash(retained[0]), hash(retained[1599]));

const withGlobalPrototype = makeRows(3);
const objectPrototype: any = Object.prototype;
objectPrototype.toJSON = function(key: string) { return 'global:' + key; };
console.log('global-prototype', JSON.stringify(withGlobalPrototype));
delete objectPrototype.toJSON;
console.log('global-restored', hash(JSON.stringify(withGlobalPrototype)));
