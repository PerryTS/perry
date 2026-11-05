// Root arrays exceed the tape threshold. Access and retain their decoded
// elements so the check includes lazy materialization and moving collections.
const lengths = [0, 1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129];
const tokens = ["0", "-0", "123456789012345678901234567890", "0.01234567890123456789",
  "-1234.125e+12", "1e400", "1e-400", "5e-324"];
const retained: any[] = [];
let failures = 0;
let hash = 0;
const decoded = "\u1234\n\"\\東京🙂";
for (let round = 0; round < 8; round++) {
  for (let i = 0; i < lengths.length; i++) {
    const pad = "a".repeat(2048 + lengths[i]);
    for (let j = 0; j < tokens.length; j++) {
      const text = '["' + pad + '",' + tokens[j] + ',"\\u1234\\n\\\"\\\\東京🙂",{"key":' + round + '},[true,false,null]]';
      const value: any = JSON.parse(text);
      if (value.length !== 5 || value[0] !== pad || value[2] !== decoded || value[3].key !== round) failures++;
      if (value[4][0] !== true || value[4][1] !== false || value[4][2] !== null) failures++;
      if (!Object.is(value[1], Number(tokens[j]))) failures++;
      const encoded = JSON.stringify(value[1]) + JSON.stringify(value[2]);
      for (let k = 0; k < encoded.length; k++) hash = (hash * 33 + encoded.charCodeAt(k)) % 1000000007;
      retained.push(value);
    }
  }
}
for (let i = 0; i < retained.length; i++) {
  if (retained[i][2] !== decoded || retained[i][4][2] !== null) failures++;
}

const bad = ["-", "01", "-01", "1.", "1e", "1e+", "1e-", "1e+-1", "1x", "1.2.3", "0x1", "1e1.0", "1e00x",
  '"\\x"', '"\\u12xz"', '"\\u123"', '"unterminated'];
let rejected = 0;
for (let i = 0; i < bad.length; i++) {
  const text = '["' + "a".repeat(2048 + i) + '",' + bad[i] + ']';
  try { JSON.parse(text); failures++; }
  catch (error: any) { if (error.name === "SyntaxError") rejected++; else failures++; }
}
for (let i = 0; i < 32; i++) {
  const text = '["' + "a".repeat(2048 + i) + String.fromCharCode(i) + '"]';
  try { JSON.parse(text); failures++; }
  catch (error: any) { if (error.name === "SyntaxError") rejected++; else failures++; }
}
console.log("tape scanning", retained.length, rejected, failures, hash);
if (failures !== 0) throw new Error("tape scanning failure");
