// Mix allocation-free scalar results with retained allocating parses. A pending
// collection from a previous object parse must not invalidate scalar decoding.
const texts = ["null", "true", "false", "0", "-0", "42", "-123", "9007199254740993",
  "18446744073709551615", "260.75197", "123456789012345678901234567890", "1e400", "-1e400",
  "1e-400", "-1e-400", "5e-324", "4.9406564584124654e-324", "1.7976931348623157e308",
  '""', '"a"', '"abcde"', '"abcdef"', '"é"', '"東京"', '"🙂"', '"éabc"',
  '"\\n"', '"\\u0061"', '"\\ud800"', '"\\udfff"', '"\\ud83d\\ude00"'];
const spaces = ["", " ", "\r\n\t", " \t\r\n "];
const retained: any[] = [];
let hash = 0;
let failures = 0;
for (let round = 0; round < 32; round++) {
  for (let i = 0; i < texts.length; i++) {
    const input = spaces[round % spaces.length] + texts[i] + spaces[i % spaces.length];
    const value: any = JSON.parse(input);
    const output = JSON.stringify(value);
    const spelling = typeof value + ":" + (Object.is(value, -0) ? "negative-zero" : output);
    for (let k = 0; k < spelling.length; k++) hash = (hash * 33 + spelling.charCodeAt(k)) % 1000000007;
    // Infinity stringifies to null, and -0 to 0; compare the canonical output.
    const parent: any = JSON.parse('{"kept":' + output + ',"round":' + round + '}');
    retained.push({ parent: parent, expected: output, value: value });
  }
}
for (let i = 0; i < retained.length; i++) {
  if (JSON.stringify(retained[i].parent.kept) !== retained[i].expected) failures++;
  if (JSON.stringify(retained[i].value) !== retained[i].expected) failures++;
}
console.log("scalar parse", retained.length, failures, hash);

const bad = ["", " ", "-", "+1", "01", "-01", "1.", "1e", "1e+", "1e-", "1e+-1", "1x",
  "1.2.3", "0x1", "1e1.0", "1e00x", "NaN", "Infinity", "-Infinity", "1 2", "1\0", "true false",
  "nullx", "\u000bnull", "null\u000c", "\u00a0true", "false\ufeff", '"a"x', '"a', '"\\x"'];
let rejected = 0;
for (let i = 0; i < bad.length; i++) {
  try { JSON.parse(bad[i]); failures++; }
  catch (error: any) { if (error.name === "SyntaxError") rejected++; else failures++; }
}
console.log("scalar invalid", rejected, failures);

let calls = "";
for (let i = 0; i < texts.length; i++) {
  const value = JSON.parse(texts[i], function(key: string, value: any) {
    calls += key + ":" + typeof value + ",";
    return { wrapped: value };
  });
  console.log(JSON.stringify(value));
}
console.log(calls);
const source: any = { toString() { calls += "coerce,"; return "123"; } };
console.log(JSON.parse(source), JSON.parse(null as any), JSON.parse(true as any), JSON.parse(12 as any));
try { JSON.parse(Symbol("bad") as any); }
catch (error: any) { console.log(error.name); }
console.log(calls.endsWith("coerce,"));
if (failures !== 0) throw new Error("scalar parse failure");
