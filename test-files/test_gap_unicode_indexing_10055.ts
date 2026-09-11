// UTF-16 indexed access must agree across forward, backward and random reads.
function check(s: string, units: number[]): void {
  if (s.length !== units.length) throw new Error("length");
  let seed = 17;
  for (let pass = 0; pass < 3; pass++) {
    for (let j = 0; j < units.length; j++) {
      // Keep the generator within positive i32 arithmetic so this fixture
      // isolates string indexing from unsigned-integer lowering.
      seed = (seed * 109 + 89) % 65521;
      const i = pass === 0 ? j : pass === 1 ? units.length - j - 1 : seed % units.length;
      const expected = units[i];
      if (s.charCodeAt(i) !== expected) throw new Error("charCodeAt");
      if (s.charAt(i).charCodeAt(0) !== expected) throw new Error("charAt");
      if (s[i].charCodeAt(0) !== expected) throw new Error("bracket");
      if (s.at(i).charCodeAt(0) !== expected) throw new Error("at");
      if (s.at(i - s.length).charCodeAt(0) !== expected) throw new Error("negative at");
    }
  }
  for (const i of [-1, s.length, s.length + 1]) {
    if (!Number.isNaN(s.charCodeAt(i))) throw new Error("out of bounds code");
    if (s.charAt(i) !== "") throw new Error("out of bounds char");
    if (s[i] !== undefined) throw new Error("out of bounds bracket");
  }
}

check("", []);
check("abc", [97, 98, 99]);
check("ä中😀Ö", [228, 20013, 55357, 56832, 214]);
check("\uD800x\uDC00", [55296, 120, 56320]);
// Concatenation also exercises the inline short-string representation.
let short = "ä";
short += "x";
check(short, [228, 120]);

const token = "Aä中😀Ö\uD800x\uDC00";
const tokenUnits = [65, 228, 20013, 55357, 56832, 214, 55296, 120, 56320];
const units: number[] = [];
for (let i = 0; i < 1024; i++) {
  for (let j = 0; j < tokenUnits.length; j++) units.push(tokenUnits[j]);
}
check(token.repeat(1024), units);

// Alternate independently allocated strings, then append to a growing string.
const strings: string[] = [];
for (let i = 0; i < 12; i++) strings.push(token.repeat(128) + String.fromCharCode(65 + i));
for (let j = 0; j < 300; j++) {
  for (let i = 0; i < strings.length; i++) {
    const s = strings[i];
    if (s.charCodeAt(s.length - 1) !== 65 + i) throw new Error("string identity");
    if (s.charCodeAt(j) !== tokenUnits[j % tokenUnits.length]) throw new Error("interleaving");
  }
}
let growing = token.repeat(32);
for (let i = 0; i < 128; i++) {
  if (growing.charCodeAt(growing.length - 1) !== 56320) throw new Error("append before");
  growing += token;
  if (growing.charCodeAt(growing.length - 1) !== 56320) throw new Error("append after");
}
console.log("unicode indexing ok");
