const shape = process.argv[2];
const mode = process.argv[3];
let source = '[';
for (let i = 0; i < 180; i++) {
    if (i) source += ',';
    source += '{"id":' + i + ',"name":"Ada","tags":[1,2]}';
}
source += ']';
const parsed: any = JSON.parse(source);
let value: any = parsed;
if (shape === "object") value = {rows: parsed, tail: "keep the parent live"};
if (shape === "array") value = [parsed, "keep the parent live"];
console.log("before");
if (mode === "callback") {
    console.log(JSON.stringify(value, function(key: string, v: any): any { return v; }));
} else if (mode === "keys") {
    console.log(JSON.stringify(value, ["rows", "tail", "id", "name", "tags"]));
} else {
    let space: any = undefined;
    if (mode === "zero") space = 0;
    if (mode === "true") space = true;
    if (mode === "pretty") space = 2;
    console.log(JSON.stringify(value, null, space));
}
console.log("done");
