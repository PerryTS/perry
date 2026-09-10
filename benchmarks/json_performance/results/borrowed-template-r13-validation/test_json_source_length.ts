const unicode = 'Grüße東京🙂'.repeat(40000);
const ascii = 'a'.repeat(525000);
const retained: any[] = [];
for (let i = 0; i < 8; i++) {
    const payload = (i % 2 === 0 ? unicode : ascii) + '-' + i + '🙂';
    const value: any = {id: i, text: payload, active: true};
    const expected = JSON.stringify(value);
    let source = expected;
    if (i === 1) source = ' \t\n' + source + '\r\n ';
    // A non-ASCII key or sibling must keep the ordinary counting fallback.
    if (i === 2) {
        value['東京'] = 'é';
        source = JSON.stringify(value);
    }
    // Escapes remain on the existing decoded-string builder path.
    if (i === 3) source = '{"id":3,"text":"' + payload + '\\n","active":true}';
    const parsed: any = JSON.parse(source);
    const wanted = i === 3 ? payload + '\n' : payload;
    if (parsed.text !== wanted || parsed.text.length !== wanted.length) {
        throw new Error('parsed payload or UTF-16 length changed');
    }
    const output = JSON.stringify(parsed);
    const canonical = i === 2 ? JSON.stringify(value) : i === 3
        ? JSON.stringify({id: 3, text: wanted, active: true}) : expected;
    if (output !== canonical) throw new Error('stringify bytes changed');
    retained.push({source: source, parsed: parsed, output: output, expected: canonical});
    const pressure: any[] = [];
    for (let j = 0; j < 120; j++) pressure.push({id: j, text: 'pressure-' + j});
    console.log('case', i, parsed.text.length, output.length, pressure[119].id);
}
for (let i = 0; i < retained.length; i++) {
    const saved: any = retained[i];
    if (JSON.stringify(saved.parsed) !== saved.expected) throw new Error('retained output changed');
    const again: any = JSON.parse(saved.source);
    if (again.text !== saved.parsed.text) throw new Error('retained source changed');
    console.log('retained', i, again.text.length, saved.output.length);
}
const scalarSource = ' \n"' + unicode + '"\t ';
const scalar: any = JSON.parse(scalarSource);
if (scalar !== unicode || scalar.length !== unicode.length) throw new Error('scalar changed');
console.log('scalar', scalar.length, JSON.stringify(scalar).length);
