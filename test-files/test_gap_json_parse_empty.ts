const first: any = JSON.parse("{}");
const second: any = JSON.parse("{}");
console.log("fresh", first !== second, Object.getPrototypeOf(first) === Object.prototype);
const spaces = ["", " ", "\t", "\n", "\r", " \t\r\n"];
let accepted = 0;
for (const prefix of spaces) {
    for (const inner of spaces) {
        for (const suffix of spaces) {
            const value = JSON.parse(prefix + "{" + inner + "}" + suffix);
            if (JSON.stringify(value) === "{}") accepted++;
        }
    }
}
let rejected = 0;
for (const bad of ["{", "}", "{}x", "{}{}", "{{}}", "{,}", "{\u00a0}", "{}\ufeff", "\u000b{}", "{}\u000c", "{}\u0000"]) {
    try { JSON.parse(bad); }
    catch (error: any) { if (error.name === "SyntaxError") rejected++; }
}
console.log("grammar", accepted, rejected);
const key = "dynamic";
first.a = 1;
first[key] = "東京🙂";
first[4] = true;
first.extra = { answer: 42 };
delete first.a;
first.a = 7;
Object.defineProperty(first, "hidden", { value: 99, enumerable: false });
Object.defineProperty(first, "getter", { get: function() { return "seen"; }, enumerable: true });
console.log("mutations", JSON.stringify(first), JSON.stringify(second));
console.log("keys", Object.keys(first).join(","));
Object.setPrototypeOf(second, { inherited: 12 });
console.log("prototype", second.inherited, JSON.stringify(second));
const third: any = JSON.parse("{}");
Object.defineProperty(Object.prototype, "emptyParseSetter", {
    configurable: true,
    set: function(value: number) { this.observed = value + 1; }
});
third.emptyParseSetter = 41;
delete (Object.prototype as any).emptyParseSetter;
console.log("setter", JSON.stringify(third));
let reviverCalls = 0;
const revived = JSON.parse(" { } ", function(key: string, value: any) {
    reviverCalls++;
    value.revised = key === "";
    return value;
});
console.log("reviver", JSON.stringify(revived), reviverCalls);
const retained: any[] = [];
for (let i = 0; i < 50000; i++) retained.push(JSON.parse(i % 2 === 0 ? "{}" : " \n{\t} "));
let failures = 0;
for (let i = 0; i < 300000; i++) {
    const value: any = JSON.parse("{}");
    if (i % 4096 === 0 && JSON.stringify(value) !== "{}") failures++;
}
let hash = 0;
for (let i = 0; i < retained.length; i++) {
    const value = retained[i];
    if (JSON.stringify(value) !== "{}") failures++;
    value.id = i;
    const output = JSON.stringify(value);
    const expected = '{"id":' + i + '}';
    if (output !== expected) failures++;
    for (let j = 0; j < output.length; j++) hash = (hash * 33 + output.charCodeAt(j)) % 1000000007;
}
console.log("retained", retained.length, failures, hash);
console.log("late", JSON.stringify(first), JSON.stringify(second), JSON.stringify(third), JSON.stringify(revived));
if (accepted !== 216 || rejected !== 11 || failures !== 0) throw new Error("empty parse failure");
