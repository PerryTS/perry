const text = '[1,' + '0,'.repeat(99998) + '7]';
const retained: any[] = [];
for (let i = 0; i < 8; i++) retained.push(JSON.parse(text));
let rejected = false;
try { JSON.parse(text.slice(0, text.length - 1)); } catch (_) { rejected = true; }
console.log('invalid', rejected, JSON.parse('[true,false,null]').length);
let hash = 0;
for (let i = 0; i < retained.length; i++) {
    const value = retained[i];
    console.log('retained', i, value.length, value[0], value[99999], value[1]);
    value[1] = i + 10;
    const result = JSON.stringify(value);
    const expected = '[1,' + String(i + 10) + ',' + '0,'.repeat(99997) + '7]';
    console.log('complete', result === expected, result.length);
    for (let j = 0; j < result.length; j++) hash = (hash * 33 + result.charCodeAt(j)) % 1000000007;
}
console.log('hash', hash);
for (let i = 0; i < 24; i++) {
    const value = JSON.parse(text);
    if (i % 8 === 0) console.log('churn', i, value.length, value[0], value[99999]);
}
const nestedText = '[' + '{"value":1},'.repeat(29999) + '{"value":1}]';
const nested = JSON.parse(nestedText);
const independent = JSON.parse(nestedText);
nested[0].value = 9;
nested[29999].value = 4;
const nestedExpected = '[{"value":9},' + '{"value":1},'.repeat(29998) + '{"value":4}]';
console.log('nested', nested.length, JSON.stringify(nested) === nestedExpected);
console.log('independent', JSON.stringify(independent) === nestedText);
const eager = JSON.parse('{"items":' + text + '}');
console.log('eager', eager.items.length, JSON.stringify(eager) === '{"items":' + text + '}');
