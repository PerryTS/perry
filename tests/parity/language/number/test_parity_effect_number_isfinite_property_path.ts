let failures = 0;
function check(label: string, actual: any, expected: any): void {
  if (actual !== expected) {
    console.log(label + ": expected " + String(expected) + ", got " + String(actual));
    failures++;
  }
}

check("direct finite", Number.isFinite(1), true);
check("direct infinity", Number.isFinite(Infinity), false);
check("direct string", Number.isFinite("1"), false);
check("global finite", globalThis.Number.isFinite(1), true);
check("global infinity", globalThis.Number.isFinite(Infinity), false);
check("global string", globalThis.Number.isFinite("1"), false);
if (failures !== 0) throw new Error("Number.isFinite property-path parity failed");
console.log("effect Number.isFinite property path ok");
