// Fractional loop-phi values and alias copies are Numbers by construction.
// Declared number parameters and captured/mutated bindings need live proofs.
function numericCopies(steps: number): number {
  let x: any = 0.5;
  let y: any = 1.25;
  for (let i = 0; i < steps; i++) {
    const next = x * 0.5 + y;
    y = x;
    x = next;
  }
  return x + y;
}
console.log("numeric", numericCopies(8));

function annotatedCopy(source: number): any {
  let target: any = 0;
  target = source;
  return target;
}
console.log("annotation", annotatedCopy("heap string passed through number" as any));
console.log("negative-zero", Object.is(annotatedCopy(-0), -0));
console.log("nan", Number.isNaN(annotatedCopy(NaN)));

function stringSnapshot(n: number): string {
  let owner: any = "";
  let snapshot: any = "";
  for (let i = 0; i < n; i++) {
    owner += "0123456789abcdefghij";
    if (i === 8) snapshot = owner;
  }
  return snapshot.length + ":" + owner.length + ":" + snapshot.slice(-20);
}
console.log("string-alias", stringSnapshot(20));

function mutatedCapture(): string {
  let source: any = 1.5;
  const replace = () => { source = "captured heap string replacement"; };
  replace();
  let copy: any = 0;
  copy = source;
  return copy;
}
console.log("capture", mutatedCapture());

function effectOrder(): string {
  const effects: string[] = [];
  const object: any = { valueOf() { effects.push("coerce"); return 1.25; } };
  let value: any = object;
  let copy: any = value;
  value = value + 2;
  return String(copy === object) + ":" + value + ":" + effects.join(",");
}
console.log("effects", effectOrder());
