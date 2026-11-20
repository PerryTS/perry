function record(): any {
    return JSON.parse('{"id":42,"name":"user_42","email":"user_42@example.com","active":false,"score":63,"tags":["tag_alpha","tag_bravo"]}');
}
let hash = 0;
const retained: string[] = [];
const input = record();
for (let i = 0; i < 1024; i++) {
    input.id = i;
    input.tags[1] = "item_" + i;
    const text = JSON.stringify(input);
    retained.push(text);
    for (let j = 0; j < text.length; j++) hash = (hash * 33 + text.charCodeAt(j)) % 1000000007;
}
console.log(retained.length, hash, retained[0], retained[1023]);
for (let n = 0; n <= 18; n++) {
    const data = record();
    data.tags = [];
    for (let i = 0; i < n; i++) data.tags.push(i / 4);
    console.log(JSON.stringify(data));
}
console.log(JSON.stringify(JSON.parse('{"a":[1,2],"b":[3,4],"c":[5,6],"d":[7,8],"e":[9,10],"f":[11,12],"g":[13,14],"h":[15,16]}')));
for (const value of [null, true, false, -0, 1e-7, 1e21, Infinity, NaN, "東京🙂", "quote\"\n", undefined]) {
    const data = record();
    data.tags[0] = value;
    console.log(JSON.stringify(data));
}
const hole = record();
delete hole.tags[0];
console.log(JSON.stringify(hole));
const getter = record();
let calls = 0;
Object.defineProperty(getter.tags, "0", { configurable: true, enumerable: true,
    get: function () { calls++; getter.score = 123; return "array getter"; } });
console.log(JSON.stringify(getter), calls);
Object.defineProperty(getter, "email", { enumerable: false });
console.log(JSON.stringify(getter), calls);
const named = record();
named.tags.toJSON = function (key: string) { return "array:" + key; };
console.log(JSON.stringify(named));
const nested = record();
nested.tags[1] = { toJSON: function (key: string) { return "nested:" + key; } };
console.log(JSON.stringify(nested));
const custom = record();
Object.setPrototypeOf(custom, { toJSON: function (key: string) { return "record:" + key; } });
console.log(JSON.stringify(custom));
console.log(JSON.stringify(record(), ["id", "tags"], 2));
console.log(JSON.stringify(record(), function (key: string, value: any) { return key === "email" ? undefined : value; }));
const indexed = record();
indexed["20"] = 20;
indexed["3"] = 3;
console.log(JSON.stringify(indexed));
