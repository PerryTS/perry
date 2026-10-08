// Ordinary indexed reads and record reads share the element backend.
class Entry {
    value: number;
    constructor(value: number) { this.value = value; }
}
const indexed: Entry[] = [new Entry(1), new Entry(2)];
const alias = indexed;
let total = 0;
for (let i = 0; i < indexed.length; i++) {
    if (i === 1) {
        for (let j = 3; j <= 40; j++) alias.push(new Entry(j));
    }
    total += indexed[i].value;
}
console.log(total, indexed.length);
let other = 0;
const record: any = [1, 2];
for (const value of record) {
    if (value === 2) for (let j = 3; j <= 40; j++) record.push(j);
    other += value;
}
console.log(other, record.length);
const holey: any = [1, , 3];
Object.defineProperty(Array.prototype, "1", {
    configurable: true, get() { return 7; }
});
let sum = 0;
for (let i = 0; i < holey.length; i++) sum += holey[i];
delete Array.prototype[1];
console.log(sum);
