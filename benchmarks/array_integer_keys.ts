const n = parseInt(process.argv[2] || "200000", 10);
const mode = process.argv[3] || "char";
const dynamicKey = parseFloat(process.argv[4] || "65");
const table: number[] = [];
for (let i = 0; i < 256; i++) table.push(i + 1);
const text = "the quick brown fox jumps over a lazy dog 0123456789";
function chars(rounds: number): number {
  let sum = 0;
  for (let k = 0; k < rounds; k++) {
    for (let i = 0; i < text.length; i++) {
      const c = text.charCodeAt(i);
      sum += table[c];
    }
  }
  return sum;
}
function surrogates(rounds: number): number {
  let sum = 0;
  for (let k = 0; k < rounds; k++) {
    for (let i = 0; i < text.length; i++) {
      let c = text.charCodeAt(i);
      sum += table[c];
      if (c >= 0xd800 && c < 0xdc00) {
        c = 0x10000 + (((c & 0x3ff) << 10) | (text.charCodeAt(++i) & 0x3ff));
        sum += table[c & 255];
      }
    }
  }
  return sum;
}
function masks(rounds: number): number {
  let sum = 0;
  for (let i = 0; i < rounds; i++) {
    const key = (i & 255) | 0;
    table[key] = key + 1;
    sum += table[key];
  }
  return sum;
}
function dynamic(rounds: number, offset: number): number {
  let sum = 0;
  for (let i = 0; i < rounds; i++) {
    const key = (i % 128) + offset;
    table[key] = i & 255;
    sum += table[key];
  }
  return sum;
}
console.log(mode === "char" ? chars(n) : mode === "surrogate" ? surrogates(n) : mode === "mask" ? masks(n) : dynamic(n, dynamicKey));
