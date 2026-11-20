declare function gc(): void;

function fresh(): any {
    return JSON.parse('{"id":42,"name":"user_42","email":"user_42@example.com","active":false,"score":63,"tags":["a","b"]}');
}

// Repeated root records exercise the no-template path, with changing values.
for (let i = 0; i < 64; i++) {
    const row = fresh();
    row.id = i;
    row.name = "東京🙂\n\"";
    row.tags = i % 2 === 0
        ? ["line\n", null, false, -0, 1.25, NaN, Infinity]
        : [undefined, null, false, -0, 1.25, NaN, Infinity];
    console.log(JSON.stringify(row));
}
const row = fresh();
row.tags.toJSON = function(key: string) {
    if (typeof gc === 'function') gc();
    return "array:" + key;
};
console.log(JSON.stringify(row));
row.tags = [1, { toJSON(key: string) { if (typeof gc === 'function') gc(); return "child:" + key; } }];
console.log(JSON.stringify(row));
row.tags = [1, 2, 3];
delete row.tags[1];
console.log(JSON.stringify(row));
row.tags = ["a", "b"];
row.id = undefined;
console.log(JSON.stringify(row));
row.id = Symbol("omitted");
console.log(JSON.stringify(row));
row.id = 42;
let reads = 0;
Object.defineProperty(row, "score", { get() { reads++; if (typeof gc === 'function') gc(); return 7; }, enumerable: true });
console.log(JSON.stringify(row));
console.log(reads);
Object.defineProperty(row, "name", { enumerable: false });
console.log(JSON.stringify(row));
const ordered = JSON.parse('{"b":1,"10":10,"2":2,"a":true,"tags":[1,2]}');
console.log(JSON.stringify(ordered));
const own = fresh();
own.toJSON = function(key: string) { if (typeof gc === 'function') gc(); return "root:" + key; };
console.log(JSON.stringify(own));
const nested = fresh();
nested.tags = [fresh(), fresh()];
console.log(JSON.stringify(nested));
