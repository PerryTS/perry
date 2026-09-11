// Candidate emitter coverage: source provenance, mutation and allocating callbacks.
const values: string[] = ['', 'a', 'abcd', 'abcde', 'abcdef', 'plain heap text', '東京🙂é', 'line\nquote"\\', '\ud800', '\udc00', '\ud800\udc00'];
function identity(key: string, value: any): any { return value; }
for (let i = 0; i < values.length; i++) {
    const value = values[i];
    const parsed: any = JSON.parse('{"text":' + JSON.stringify(value) + ',"short":"abcd"}');
    console.log('pretty', i, JSON.stringify(parsed, null, 2));
    console.log('callback', i, JSON.stringify(parsed, identity));
    console.log('callback-pretty', i, JSON.stringify(parsed, identity, 2));
    parsed.text = parsed.text + '\n"';
    parsed.short = parsed.short + '\\';
    console.log('mutated-pretty', i, JSON.stringify(parsed, null, 2));
    console.log('mutated-callback', i, JSON.stringify(parsed, identity));
}
const retained: string[] = [];
let checksum = 0;
function allocateString(key: string, value: any): any {
    if (key === 'text') {
        const churn: any[] = [];
        for (let i = 0; i < 48; i++) churn.push({id: i, text: 'callback survivor ' + i});
        const parsed: any = JSON.parse('{"text":"callback string ' + churn[47].id + '"}');
        return parsed.text;
    }
    if (key === 'short') return 'a' + 'bcd';
    return value;
}
for (let i = 0; i < 64; i++) {
    const parsed: any = JSON.parse('{"id":' + i + ',"text":"source token","short":"short"}');
    const output: string = JSON.stringify(parsed, allocateString, i % 2 === 0 ? 0 : 2);
    retained.push(output);
    checksum += output.length;
}
for (let i = 0; i < retained.length; i++) {
    const parsed: any = JSON.parse(retained[i]);
    if (parsed.id !== i || parsed.text !== 'callback string 47' || parsed.short !== 'abcd') throw new Error('retained callback output changed');
    checksum += parsed.text.length;
}
console.log('retained', retained.length, checksum);
