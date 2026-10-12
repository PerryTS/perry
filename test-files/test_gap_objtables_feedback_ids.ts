class Row { value: number; constructor(value: number) { this.value = value; } read() { return this.value; } }
const rows = [new Row(11), new Row(29)];
console.log(rows[0].read(), rows[1].read());
for (let i = 0; i < 50000; i++) { const x = { i, text: 'feedback' + i }; if (x.i < 0) console.log(x); }
console.log(rows[0].read(), rows[1].read());
const other = { value: 41, read() { return this.value; } };
console.log(other.read());
