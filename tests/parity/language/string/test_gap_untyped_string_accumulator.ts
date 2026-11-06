// Untyped string accumulators, the shape of TypeScript's createTextWriter:
// the binding is declared without a type (var output), reset to the empty
// string and grown with output += s from sibling closures, and measured with
// output.length between appends. It takes the in-place append; every ordinary
// read must still see an unaliased value, and a length read the current length.

function createWriter(): any {
  var output: any;
  var linePos = 0;
  var lineCount = 0;
  function reset() { output = ""; linePos = 0; lineCount = 0; }
  function writeText(s: any) { if (s && s.length) { output += s; } }
  function writeLine() { output += "\n"; lineCount++; linePos = output.length; }
  function getText() { return output; }
  function getTextPos() { return output.length; }
  function getColumn() { return output.length - linePos; }
  reset();
  return { writeText, writeLine, getText, getTextPos, getColumn, reset, lines: () => lineCount };
}

const w = createWriter();
w.writeText("ab");
const pos0 = w.getTextPos();
w.writeText("cd");
const snap = w.getText();
w.writeText("ef");
const pos1 = w.getTextPos();
w.writeLine();
w.writeText("gh");
console.log(pos0, JSON.stringify(snap), pos1, JSON.stringify(w.getText()), w.getColumn(), snap.length);

// Snapshots taken between appends keep their own contents.
const snaps: string[] = [];
w.reset();
for (let i = 0; i < 6; i++) {
  w.writeText(String(i));
  if (i % 2 === 0) snaps.push(w.getText());
  w.getTextPos();
}
console.log(snaps.join(","), w.getText());

// A long build: amortized, and the result is exact.
w.reset();
let expected = 0;
for (let i = 0; i < 20000; i++) {
  const s = "tok" + (i % 10);
  w.writeText(s);
  expected += s.length;
  if (i % 8 === 0) { w.writeLine(); expected += 1; }
}
const text = w.getText();
let checksum = 0;
for (let i = 0; i < text.length; i += 997) checksum = (checksum * 31 + text.charCodeAt(i)) | 0;
console.log(text.length === expected, text.length, w.lines(), checksum, text.slice(0, 12), text.slice(-6));

// The same binding can stop being a string:  must keep JS semantics.
var acc: any = "";
function grow(x: any) { acc = acc + x; }
grow(1); grow("a"); grow(null); grow(undefined); grow(true);
console.log(JSON.stringify(acc));
acc = 5;
grow(2);
console.log(acc);
acc = "";
grow({ toString() { return "T"; } });
grow([1, 2]);
console.log(JSON.stringify(acc));

// A self-add on a binding never given a string stays numeric.
var n: any = 0;
function bump() { n = n + 1; }
for (let i = 0; i < 10; i++) bump();
console.log(n);
