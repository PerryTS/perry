function makeWide(): any {
    let text = "{";
    for (let i = 0; i < 128; i++) {
        if (i > 0) text += ",";
        text += '"field_' + i + '":' + i;
    }
    return JSON.parse(text + "}");
}

const input = makeWide();
input["20"] = 20;
input["3"] = 3;
input['quote"'] = 'line\n東京🙂';
input.field_127 = undefined;
console.log(JSON.stringify(input));
console.log(JSON.stringify({ data: input }));
console.log(JSON.stringify([input, input]));

const retained: string[] = [];
for (let i = 0; i < 64; i++) {
    input.field_0 = i;
    retained.push(JSON.stringify(input));
}
console.log(retained[0]);
console.log(retained[63]);

let calls = 0;
Object.defineProperty(input, "field_64", {
    enumerable: true,
    configurable: true,
    get: function () { calls++; return "getter"; }
});
Object.defineProperty(input, "field_65", { enumerable: false });
console.log(JSON.stringify(input));
console.log(calls);

const late = makeWide();
late.field_127 = { toJSON: function (key: string) { return "key:" + key; } };
console.log(JSON.stringify(late));
late.field_127 = { toJSON: function () { return undefined; } };
console.log(JSON.stringify(late));

const proto: any = Object.prototype;
proto.toJSON = function (key: string) { return "prototype:" + key; };
console.log(JSON.stringify(makeWide()));
delete proto.toJSON;
console.log(JSON.stringify(makeWide()));

const custom = makeWide();
Object.setPrototypeOf(custom, { toJSON: function () { return "custom"; } });
console.log(JSON.stringify(custom));
console.log(JSON.stringify(makeWide(), ["field_1", "field_127"], 2));
