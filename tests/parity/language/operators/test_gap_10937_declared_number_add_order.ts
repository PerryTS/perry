// #10937: annotations cannot license reading a later operand before valueOf.
// #10921 already fixed both fold entries; these controls pin the distinction
// between a late read and an intentional snapshot, including conversion count.
let failures = 0;
let checks = 0;
function check(name: string, actual: number, expected: number, calls: number, late: number) {
  checks++;
  if (actual !== expected || calls !== 1 || late !== 100) {
    console.log("FAIL", name, "result", actual, "expected", expected, "conversions", calls, "late", late);
    failures++;
  } else {
    console.log("PASS", name, actual, calls, late);
  }
}
function leftElements(a: number[]): number { return a[0] + a[1] + a[2]; }
function rightElements(a: number[]): number { return a[0] + (a[1] + a[2]); }
function snapshotElements(a: number[]): number {
  const x = a[0], y = a[1], z = a[2];
  return x + y + z;
}
function elements(mode: number) {
  const a: number[] = [1, 2, 3];
  let calls = 0;
  (a as any)[0] = { valueOf() { calls++; a[2] = 100; return 1; } };
  if (mode === 0) check("elements left", leftElements(a), 103, calls, a[2]);
  if (mode === 1) check("elements right", rightElements(a), 6, calls, a[2]);
  if (mode === 2) check("elements snapshot", snapshotElements(a), 6, calls, a[2]);
}
class Holder10937 {
  a: number = 1; b: number = 2; c: number = 3; e: number = 4;
  left(): number { return this.a + this.b + this.c + this.e; }
  right(): number { return this.a + (this.b + (this.c + this.e)); }
  snapshot(): number {
    const a = this.a, b = this.b, c = this.c, e = this.e;
    return a + b + c + e;
  }
}
function fields(mode: number) {
  const h = new Holder10937();
  let calls = 0;
  (h as any).a = { valueOf() { calls++; h.c = 100; return 1; } };
  if (mode === 0) check("this left", h.left(), 107, calls, h.c);
  if (mode === 1) check("this right", h.right(), 10, calls, h.c);
  if (mode === 2) check("this snapshot", h.snapshot(), 10, calls, h.c);
}
elements(0);
elements(1);
elements(2);
fields(0);
fields(1);
fields(2);
if (checks !== 6) throw new Error("#10937 must execute all six cases");
if (failures !== 0) throw new Error("#10937 declared-number addition order failures: " + failures);
