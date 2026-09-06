// Candidate coverage for callback-free record emission. Compare exact output.
const records: any[] = [];
for (let i = 0; i < 48; i++) {
    const row = JSON.parse('{"id":1,"name":"a","active":true,"score":1.5,"tags":["x","y"]}');
    row.id = i;
    row.name = i % 2 === 0 ? "東京🙂" : "line\n\"quote\"";
    row.score = i % 3 === 0 ? -0 : i * 0.125;
    row.tags = ["tag" + i, null, false, i * 1.5];
    records.push(row);
}
console.log(JSON.stringify(records));
console.log(JSON.stringify({ records }));
const varied = JSON.parse('[{"id":1,"tags":[1,2]},{"id":2,"tags":[3,4]}]');
varied[1].tags.toJSON = function(key: string) { return "array:" + key; };
console.log(JSON.stringify(varied));
varied[1].tags = [undefined, NaN, Infinity, -Infinity];
console.log(JSON.stringify(varied));
varied[1].tags = ["x", { toJSON(key: string) { return "child:" + key; } }];
console.log(JSON.stringify(varied));
varied[1].tags = [1, 2];
varied[1].id = undefined;
console.log(JSON.stringify(varied));
varied[1].id = function() { return 7; };
console.log(JSON.stringify(varied));
varied[1].id = Symbol("omitted");
console.log(JSON.stringify(varied));
varied[1].id = 2;
Object.defineProperty(varied[1], "id", { enumerable: false });
console.log(JSON.stringify(varied));
let accesses = 0;
Object.defineProperty(varied[1], "id", { enumerable: true, get() { accesses++; return 99; } });
console.log(JSON.stringify(varied));
console.log(accesses);
