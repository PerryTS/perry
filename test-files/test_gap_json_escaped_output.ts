let hash = 0;
const retained: string[] = [];
function inspect(value: any): void {
    const text = JSON.stringify(value);
    retained.push(text);
    for (let i = 0; i < text.length; i++) hash = (hash * 33 + text.charCodeAt(i)) % 1000000007;
    console.log(text.length, hash, text.length < 240 ? text : "large");
}
for (let code = 0; code < 128; code++) {
    const value = "prefix" + String.fromCharCode(code) + "東京🙂suffix";
    inspect(value);
    const quoted = JSON.stringify(value);
    inspect(JSON.parse("{" + quoted + ":" + quoted + ",\"id\":1}"));
    inspect(JSON.parse("{" + quoted + ":[" + quoted + ",1,true,null],\"id\":1}"));
}
for (const value of ["\n", "\"", "\\", "\u0000", "\u000b", "\u001f", "\ud800", "\udfff", "\ud83d\udc4d", "\ud800\n\udc00"]) {
    inspect(value);
    inspect(JSON.parse("{\"value\":" + JSON.stringify(value) + "}"));
}
for (const count of [0, 1, 7, 16, 63, 64, 65, 127, 128, 1024, 45000]) {
    const value = "line\n\"quote\"\\tab\t東京🙂\u0000".repeat(count);
    inspect(value);
    inspect(JSON.parse("{\"id\":1,\"text\":" + JSON.stringify(value) + "}"));
}
const mutable: any = JSON.parse('{"id":1,"text":"initial text"}');
for (let i = 0; i < 1024; i++) {
    mutable.id = i;
    mutable.text = "entry" + i + "\n\"\\東京🙂";
    inspect(mutable);
}
const callbacks: any = JSON.parse('{"id":1,"text":"line\\nquoted\\\"text"}');
let calls = 0;
Object.defineProperty(callbacks, "text", { enumerable: true, configurable: true, get: function () { calls++; return "getter\n\"\\東京🙂"; } });
inspect(callbacks);
console.log(calls);
console.log(JSON.stringify(callbacks, ["text"], 2));
console.log(JSON.stringify(callbacks, function (key: string, value: any) { return key === "text" ? "replaced\n\"" : value; }));
Object.setPrototypeOf(callbacks, { toJSON: function (key: string) { calls++; return "prototype\n\"" + key; } });
inspect(callbacks);
console.log(calls);
let retainedHash = 0;
for (let i = 0; i < retained.length; i++) {
    const text = retained[i];
    for (let j = 0; j < text.length; j++) retainedHash = (retainedHash * 33 + text.charCodeAt(j)) % 1000000007;
}
console.log(retained.length, hash, retainedHash, hash === retainedHash);
