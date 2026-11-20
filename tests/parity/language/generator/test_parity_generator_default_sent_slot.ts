// A default resume must overwrite the sent slot even when the preceding
// resume sent a managed object. for-of uses the same default resume path.
const marker = { value: 37 };
function* sentValues() {
    const first = yield 1;
    console.log(first === marker, (first as any).value);
    const second = yield 2;
    console.log(second === undefined);
    const third = yield 3;
    console.log(third === marker);
    const fourth = yield 4;
    console.log(fourth === undefined);
    yield 5;
}
const iter = sentValues();
console.log(iter.next().value);
console.log(iter.next(marker).value);
console.log(iter.next().value);
console.log(iter.next(marker).value);
for (const value of iter) console.log(value);
console.log(iter.next().done);
