// Large borrowed tokens, escaped fallbacks and retained source/output lifetimes.
const units: string[] = ['ascii text ', '東京🙂é', '𐀀𐐷𤭢'];
const sources: string[] = [];
const expected: string[] = [];
for (let i = 0; i < units.length; i++) {
    const text = units[i].repeat(20000);
    sources.push('{"id":' + i + ',"text":"' + text + '"}');
    expected.push(text);
    sources.push('{"é":' + i + ',"text":"' + text + '"}');
    expected.push(text);
    sources.push('{"id":' + i + ',"text":"' + text + '\\n\\uD83D\\uDE42"}');
    expected.push(text + '\n🙂');
    sources.push(' '.repeat(257) + '{"id":' + i + ',"text":"' + text + '"}');
    expected.push(text);
}
const retained: any[] = [];
let checksum = 0;
for (let round = 0; round < 64; round++) {
    const index = round % sources.length;
    const parsed: any = JSON.parse(sources[index]);
    if (parsed.text !== expected[index]) throw new Error('parsed token changed');
    checksum += parsed.text.length;
    retained.push(parsed);
    // Small live objects give scheduled copying GC real nursery survivors
    // even when the large text allocations use the existing malloc path.
    const churn: any[] = [];
    for (let i = 0; i < 48; i++) {
        churn.push(JSON.parse('{"id":' + i + ',"name":"source token lifetime"}'));
    }
    if (churn[47].id !== 47) throw new Error('churn changed');
}
for (let round = 0; round < retained.length; round++) {
    const index = round % sources.length;
    const parsed: any = retained[round];
    if (parsed.text !== expected[index]) throw new Error('retained output changed');
    const again: any = JSON.parse(sources[index]);
    if (again.text !== parsed.text) throw new Error('retained source changed');
    checksum += again.text.length;
}
for (let i = 0; i < sources.length; i++) {
    const parsed: any = JSON.parse(sources[i]);
    // Full bytes, including the escaped case, are checked against Node.
    console.log('VERIFY', i, parsed.text.length, JSON.stringify(parsed));
}
console.log('retained', retained.length, checksum);
