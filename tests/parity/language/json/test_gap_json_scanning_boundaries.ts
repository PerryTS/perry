// Exercise parse/stringify scanning across word/vector tails and long strings.
// The output hash compares the actual serialization with Node, not just a
// Perry stringify/parse roundtrip that could hide matching bugs in both.
const lengths = [0, 1, 7, 8, 9, 15, 16, 17, 31, 32, 63, 64, 127, 1024, 65536];
const inserts = ["x", "\x00", "\x01", "\x1f", "\b", "\t", "\n", "\f", "\r", '"', "\\", "東京🙂", "\ud000", "\ud7ff", "\ud800", "\udfff", "\ud83d\ude00"];
let hash = 0;
let cases = 0;
let failures = 0;
for (let a = 0; a < lengths.length; a++) {
  for (let b = 0; b < inserts.length; b++) {
    const text = "a".repeat(lengths[a]) + inserts[b] + "z".repeat(17);
    const encoded = JSON.stringify({ text });
    const decoded: any = JSON.parse(encoded);
    if (decoded.text !== text) failures++;
    for (let i = 0; i < encoded.length; i++) {
      hash = (hash * 33 + encoded.charCodeAt(i)) % 1000000007;
    }
    cases++;
  }
}
console.log("scan", cases, failures, hash);

// Same names and values after decoding escapes; insertion order must survive
// duplicate replacement on either side of inline spill and hash-index creation.
let wideFailures = 0;
let wideHash = 0;
for (const count of [7, 8, 9, 16, 31, 32, 33, 64, 127, 128, 129, 130, 1024]) {
  let text = "{";
  for (let i = 0; i < count; i++) {
    if (i > 0) text += ",";
    text += '"k' + i + '":' + i;
  }
  text += ',"k0":-1,"k8":-8,"k31":-31,"k32":-32,"\\u006b0":-2';
  if (count >= 127) text += ',"k127":-127,"k128":-128,"\\u006b127":-1270';
  text += ',"__proto__":1,"__proto__":2,"2":"two","1":"one"}';
  const parsed: any = JSON.parse(text);
  if (parsed.k0 !== -2 || parsed.k8 !== -8 || parsed.k31 !== -31 || parsed.k32 !== -32) wideFailures++;
  if (count >= 127 && (parsed.k127 !== -1270 || parsed.k128 !== -128)) wideFailures++;
  const encoded = JSON.stringify(parsed);
  for (let i = 0; i < encoded.length; i++) wideHash = (wideHash * 33 + encoded.charCodeAt(i)) % 1000000007;
}
console.log("wide", wideFailures, wideHash);
