// #10061: materialized slices and scalar suffix consumers must agree with Node.
function inspect(input: string): string {
  let s = input;
  let out = "";
  for (let i = 0; i < 16 && s.length; i++) {
    out += s.length + ":" + s.charCodeAt(0) + ",";
    s = s.slice(1);
  }
  return out;
}
console.log(inspect("ä中😀Ö"));
console.log(inspect("\ud800A😀\udfff"));
console.log(inspect(""));

const input = "ä中😀Ö\ud800A\udfff";
for (let start = -11; start <= 11; start++) {
  for (let end = -11; end <= 11; end++) {
    const part = input.slice(start, end);
    console.log(start, end, part.length, JSON.stringify(part), part.isWellFormed());
  }
}
console.log("😀".substring(0, 1).charCodeAt(0), "😀".substr(1, 1).charCodeAt(0));

// Escapes and aliases require ordinary flat strings.
let current = "ä中😀Ö";
const retained: string[] = [];
while (current.length) {
  retained.push(current);
  current = current.slice(1);
}
for (let i = 0; i < retained.length; i++) console.log(JSON.stringify(retained[i]));
let captured = "😀Ö";
const read = () => captured;
captured = captured.slice(1);
console.log(JSON.stringify(read()));
let builder = "";
builder += "😀";
let alias = builder;
builder += "X";
console.log(inspect(alias), builder);

// Different strides, out-of-range reads, and repeated slicing after exhaustion.
function stride(input: string): string {
  let s = input;
  let result = "";
  for (let i = 0; i < 8; i++) {
    result += s.length + ":" + s.charCodeAt(0) + ":" + s.charCodeAt(1) + ",";
    s = s.slice(2);
  }
  return result;
}
console.log(stride("ä中😀Ö😀"));
// A string annotation must not erase a non-string receiver's own methods.
const custom: any = {
  length: 3,
  charCodeAt: (i: number) => 70 + i,
  slice: (_: number) => "xy",
};
console.log(stride(custom));
