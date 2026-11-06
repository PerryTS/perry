const retained: any[] = [];
let errors = 0;
let hash = 0;
const spaces = ["", " ", "\t\r\n"];
for (let round = 0; round < 128; round++) {
    const items: number[] = [];
    for (let i = 0; i < 512; i++) items.push(round + i);
    const text = JSON.stringify(items);
    const padded = spaces[round % spaces.length] + text + spaces[(round + 1) % spaces.length];
    const value = JSON.parse(padded);
    retained.push({ text: text, value: value });
    // Invalid tape input must fall back with roots/suppression restored, so
    // a subsequent valid parse and retained output remain usable.
    for (const bad of [text + "x", text.slice(0, -1) + ",]", "{" + text]) {
        try { JSON.parse(bad); }
        catch (error: any) { if (error.name === "SyntaxError") errors++; }
    }
    const one = JSON.parse('{"yes":true}');
    const out = JSON.stringify({ nested: one, round: round });
    for (let i = 0; i < out.length; i++) hash = (hash * 33 + out.charCodeAt(i)) % 1000000007;
}
let failures = 0;
for (let i = 0; i < retained.length; i++) {
    if (JSON.stringify(retained[i].value) !== retained[i].text) failures++;
}
console.log(retained.length, errors, failures, hash);
console.log(JSON.stringify([{ yes: true }, { yes: undefined }, { yes: "東京🙂" }]));
let calls = 0;
const leaf: any = { yes: true };
Object.defineProperty(leaf, "yes", { enumerable: true, configurable: true,
    get: function () { calls++; return "getter"; } });
console.log(JSON.stringify({ nested: leaf }), calls);
Object.defineProperty(leaf, "yes", { enumerable: false });
console.log(JSON.stringify({ nested: leaf }), calls);
const custom: any = JSON.parse('{"yes":true}');
Object.setPrototypeOf(custom, { toJSON: function (key: string) { return "custom:" + key; } });
console.log(JSON.stringify({ nested: custom }));
console.log(JSON.stringify({ nested: { toJSON: function (key: string) { return key; } } }));
console.log(JSON.stringify({ nested: { yes: true } }, null, 2));
if (failures !== 0 || errors !== 384) throw new Error("parse entry failure");
