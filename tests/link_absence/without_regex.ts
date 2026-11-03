// Ordinary string paths must work with no regex install selected.
const text: any = "ab-ab";
console.log(text.replace("ab", "[$&]"));
console.log(text.replaceAll("ab", (s: string, i: number) => s + i));
console.log(text.split("-").join(":"));
console.log(String.prototype.replace.call("aba", "a", "$`<$&>$'"));
console.log(String.prototype.split.call("a-b-c", "-", 2).join(":"));
const custom = {
  [Symbol.replace](value: any, replacement: any) { return "replace:" + value + ":" + replacement; },
  [Symbol.split](value: any, limit: any) { return "split:" + value + ":" + limit; }
};
console.log(text.replace(custom, "ok"));
console.log(text.replaceAll(custom, "ok"));
console.log(text.split(custom, 2));
console.log("😀".replaceAll("", ":").length);
console.log("😀".replace("\ud83d", "!").charCodeAt(1));
console.log("😀".replaceAll("\ude00", "!").charCodeAt(0));
console.log("\ud800".replace("\ud800", "ok"));
console.log("aaa".replaceAll("aa", "X"));
console.log("aaaaab".replaceAll("aaab", "X"));
console.log("😀x😀".split("\ude00").map((s: string) => s.length).join(":"));
const callback = new Proxy(function () {}, {
  apply(_target: any, _this: any, args: any[]) { return args[0] + args[1]; }
});
console.log(text.replaceAll("ab", callback));
const method = new Proxy(function () {}, {
  apply(_target: any, self: any, args: any[]) { return self.tag + ":" + args[0] + ":" + args[1]; }
});
console.log(text.replace({ tag: "owner", [Symbol.replace]: method }, "ok"));
console.log(text.split({ tag: "owner", [Symbol.split]: method }, 2));
const order: string[] = [];
const receiver = { toString() { order.push("receiver"); return "a-b"; } };
const separator = { toString() { order.push("separator"); return "-"; } };
const limit = { valueOf() { order.push("limit"); return 2; } };
console.log(String.prototype.split.call(receiver, separator, limit).join(":"));
console.log(order.join(","));
