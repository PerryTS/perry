// Compare UTF-16 lengths, code units and emitted JSON with Node across SIMD
// boundaries. Include astral characters and lone surrogates in live outputs.
const chunks = ["a", "é", "中", "🙂", "\ud7ff", "\ue000", "\ud800", "\udfff"];
const sizes = [0, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 1024];
let failures = 0;
let cases = 0;
let hash = 0;
const retained: any[] = [];
for (let c = 0; c < chunks.length; c++) {
  for (let n = 0; n < sizes.length; n++) {
    const text = "x".repeat(n) + chunks[c].repeat(sizes[n]) + "🙂é";
    const encoded = JSON.stringify({ text });
    const decoded: any = JSON.parse(encoded);
    if (decoded.text !== text || decoded.text.length !== text.length) failures++;
    const scalar = JSON.stringify(text);
    const scalarDecoded: any = JSON.parse(scalar);
    if (scalarDecoded.length !== text.length || scalarDecoded !== text) failures++;
    for (let i = 0; i < encoded.length; i++) {
      hash = (hash * 33 + encoded.charCodeAt(i)) % 1000000007;
    }
    retained.push(decoded);
    cases++;
  }
}
// Force a materialized array/object walk after the original calls return.
const roundtrip = JSON.stringify({ retained });
const restored: any = JSON.parse(roundtrip);
for (let i = 0; i < retained.length; i++) {
  if (restored.retained[i].text !== retained[i].text) failures++;
}
console.log("unicode lengths", cases, failures, hash, roundtrip.length);
