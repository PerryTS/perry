// Alternate inputs so the single-source string cache cannot hide allocations.
// Keep every result alive across output-budget collections, then mutate each
// object independently and check its large ASCII/Unicode string afterwards.
const ascii = 'abcdefgh'.repeat(128 * 1024);
const unicode = 'é🙂'.repeat(128 * 1024);
const sources = [
    '{"id":0,"text":"' + ascii + '"}',
    '{"id":1,"text":"' + unicode + '"}',
    '{"id":2,"text":"' + ascii + '"}',
    '{"id":3,"text":"' + unicode + '"}',
];
const records: any[] = [];
for (let i = 0; i < 96; i++) {
    records.push(JSON.parse(sources[i & 3]));
}
console.log('distinct', records[0] !== records[4], records[1] !== records[5]);
for (let i = 0; i < records.length; i++) {
    records[i].id += 1000 + i;
}
let ids = 0;
let units = 0;
let ends = 0;
for (let i = 0; i < records.length; i++) {
    const record = records[i];
    const text: string = record.text;
    ids += record.id;
    units += text.length;
    ends += text.charCodeAt(0) + text.charCodeAt(text.length - 1);
}
console.log('retained', records.length, ids, units, ends);
console.log('inputs', sources.length, ascii.length, unicode.length);
