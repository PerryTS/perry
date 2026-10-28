// NumberToString and inline/heap strings across construction boundaries.
for (const value of [0, -0, 99999, 100000, -2147483648, 4294967296,
                    1e-7, 1e-6, 1e20, 1e21, 0.1, NaN, Infinity, -Infinity]) {
    console.log("n=" + value + ";é🙂");
}
const high = String.fromCharCode(0xd83d);
const low = String.fromCharCode(0xde00);
for (const value of [high + "" + low, high + 0 + low, "é" + 42 + "🙂"]) {
    console.log(value.length, value.charCodeAt(0), value.charCodeAt(value.length - 1));
}
let conversions = 0;
const custom = { toString() { conversions++; return "custom"; } };
console.log("[" + custom + ":" + custom + "]", conversions);
const savedUpper = String.prototype.toUpperCase;
String.prototype.toUpperCase = function() { return "override"; };
console.log(("a" + 1).toUpperCase());
String.prototype.toUpperCase = savedUpper;
console.log(("éß" + 1).toUpperCase());
console.log(("a" + 1 + ";b").split(";", 1).join("|"));
console.log(("a" + 1 + ";b").split(/(;)/).join("|"));
const parsed = JSON.parse('[{"score":7},{}]');
console.log(parsed.length, parsed[0].score, parsed[1].score);
let reads = 0;
const getter = { get score() { reads++; return 9; } };
console.log(JSON.stringify([getter]), reads);
