let text = '[';
for (let i = 0; i < 256; i++) {
    if (i) text += ',';
    text += '{"id":' + i + ',"name":"row"}';
}
text += ']';
const rows: any = JSON.parse(text);
console.log(rows.length, rows[0].id, rows[255].id);
rows[1].id = 900;
Object.defineProperty(rows, '2', {value: {id: 700, name: 'changed'}});
console.log(rows[1].id, rows[2].id, JSON.stringify(rows).length);
console.log(JSON.parse('{"value":4}', (key, value) => key === 'value' ? value + 1 : value).value);
