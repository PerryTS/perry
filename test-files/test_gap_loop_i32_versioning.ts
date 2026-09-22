// One preheader representation decision; doubles are materialized at uses/exits.
function counted(n: any) {
  let h = 0;
  for (var k = 0; k < n; k++) h += 3;
  console.log(h, k);
}
counted(5);
counted(3.5);
counted(-1);
counted(NaN);
counted("4");
counted({ valueOf() { return 3; } });

function boundary(n: any) {
  let h = 0;
  for (var k = 2147483645; k < n; k++) h += k - 2147483645;
  console.log(h, k);
}
boundary(2147483647);
boundary(2 ** 31 + 5);

function readArgument(x: number) { return arguments[0]; }
function reads(n: any) {
  let h = 0;
  for (var k = 0; k < n; k++) h += readArgument(k);
  console.log(h, k);
}
reads(5);
reads(3.5);

function captured(n: any) {
  const readers: (() => number)[] = [];
  for (let k = 0; k < n; k++) readers.push(() => k);
  console.log(readers[0](), readers[1](), readers[4]());
}
captured(5);

function caught(n: any) {
  try {
    for (var k = 0; k < n; k++) console.log(fail(k));
  } catch (e) { console.log("caught", k); }
}
function fail(k: number) { if (k === 2) throw "stop"; return k; }
caught(5);

function modified(n: any) {
  let h = 0;
  for (var k = 0; k < n; k++) { k++; h += k; }
  console.log(h, k);
}
modified(5);

function privateCounter(n: any) {
  let h = 0;
  for (let k = 0; k < n; k++) h += 3;
  console.log(h);
}
privateCounter(5);
privateCounter(3.5);
privateCounter(NaN);
privateCounter("4");
function privateBoundary(n: any) {
  let h = 0;
  for (let k = 2147483645; k < n; k++) h += readArgument(k) - 2147483645;
  console.log(h);
}
privateBoundary(2147483647);
privateBoundary(2 ** 31 + 5);
