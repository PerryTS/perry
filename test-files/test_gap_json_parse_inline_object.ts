const first: any = JSON.parse('{"a":1,"b":true}');
const second: any = JSON.parse('{"a":2,"b":false}');
console.log("fresh", first !== second, JSON.stringify(first), JSON.stringify(second));
first.a = 7;
delete first.b;
first.c = "new";
console.log("independent", JSON.stringify(first), JSON.stringify(second), JSON.stringify(JSON.parse('{"a":3,"b":true}')));
let failed = 0;
let checksum = 0;
const retained: any[] = [];
for (let i = 0; i < 40000; i++) {
    const text = '{"a":' + i + ',"b":true,"c":"é"}';
    const value: any = JSON.parse(text);
    if (value.a !== i || value.b !== true || value.c !== "é") failed++;
    if (i % 8 === 0) retained.push(value);
    checksum = (checksum + value.a) % 1000000007;
}
for (let i = 0; i < retained.length; i++) {
    if (retained[i].a !== i * 8 || retained[i].c !== "é") failed++;
    retained[i].extra = i;
}
console.log("values", failed, checksum, retained.length, retained[retained.length - 1].extra);
console.log("duplicate", JSON.stringify(JSON.parse('{"b":1,"a":2,"b":3}')));
console.log("numeric", JSON.stringify(JSON.parse('{"9":1,"2":2,"x":3}')));
console.log("zero", Object.is(JSON.parse('{"a":-0}').a, -0));
let setters = 0;
Object.defineProperty(Object.prototype, "a", { configurable: true, set: function(value: any) { setters++; } });
const own: any = JSON.parse('{"a":42}');
delete (Object.prototype as any).a;
console.log("own", setters, own.a, Object.getPrototypeOf(own) === Object.prototype);
let reviverCalls = 0;
const revived = JSON.parse('{"a":1,"b":true}', function(key: string, value: any) {
    reviverCalls++;
    return key === "a" ? value + 1 : value;
});
console.log("reviver", reviverCalls, JSON.stringify(revived));
let rejected = 0;
for (const bad of ['{"a":1,}', '{"a":01}', '{"a":1}x', '{"a":truex}', '{"a":1 2}', '{"a":}', '{"a":1e}']) {
    try { JSON.parse(bad); }
    catch (error: any) { if (error.name === "SyntaxError") rejected++; }
}
console.log("invalid", rejected);
// Fill the bounded schema cache, then verify uncached and reordered shapes.
for (let i = 0; i < 300; i++) {
    const key = "k" + i;
    const value: any = JSON.parse('{"' + key + '":' + i + '}');
    if (value[key] !== i) failed++;
}
console.log("cache", failed, JSON.stringify(JSON.parse('{"b":false,"a":4}')));
if (failed !== 0 || rejected !== 7) throw new Error("inline object parse failure");
