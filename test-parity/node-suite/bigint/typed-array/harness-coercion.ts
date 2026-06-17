// @ts-nocheck

const constructors: any[] = [
  Int8Array,
  Uint8Array,
  Uint8ClampedArray,
  Int16Array,
  Uint16Array,
  Int32Array,
  Uint32Array,
  Float16Array,
  Float32Array,
  Float64Array,
  BigInt64Array,
  BigUint64Array,
];

function isBigIntCtor(ctor: any): boolean {
  return ctor === BigInt64Array || ctor === BigUint64Array;
}

function sampleValues(ctor: any): any[] {
  return isBigIntCtor(ctor) ? [1n, 2n, 3n] : [1, 2, 3];
}

function replacementValue(ctor: any): any {
  return isBigIntCtor(ctor) ? 9n : 9;
}

function viewValues(view: any): string {
  const parts: string[] = [];
  for (let i = 0; i < view.length; i++) {
    parts.push(String(view[i]) + ":" + typeof view[i]);
  }
  return parts.join("|");
}

function showView(label: string, view: any): void {
  console.log(label + ":" + view.length + ":" + viewValues(view));
}

function showThrow(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ":NO_THROW");
  } catch (error) {
    console.log(label + ":" + error.constructor.name + " - " + error.message);
  }
}

for (const Ctor of constructors) {
  const name = Ctor.name;
  const lengthView = new Ctor(3);
  console.log(
    "isView:" +
      name +
      ":" +
      ArrayBuffer.isView(lengthView) +
      ":" +
      lengthView.length +
      ":" +
      typeof lengthView[0],
  );

  const fromArray = new Ctor(sampleValues(Ctor));
  showView("fromArray:" + name, fromArray);
  showView("copyWithin:" + name, fromArray.copyWithin(1, 0, 2));
  showView("map:" + name, fromArray.map((value: any) => value));
  showView("with:" + name, fromArray.with(0, replacementValue(Ctor)));
  const filled = new Ctor(sampleValues(Ctor));
  showView("fill:" + name, filled.fill(replacementValue(Ctor), 1));
}

showThrow("BigInt64Array number array", () => {
  new BigInt64Array([1, 2] as any);
});
showThrow("BigInt64Array numeric TA source", () => {
  new BigInt64Array(new Uint8Array([1, 2]) as any);
});
showThrow("BigInt64Array set numeric TA", () => {
  const target = new BigInt64Array(2);
  target.set(new Uint8Array([1, 2]) as any);
});
showThrow("Uint8Array bigint TA source", () => {
  new Uint8Array(new BigInt64Array([1n, 2n]) as any);
});
showThrow("Uint8Array set bigint TA", () => {
  const target = new Uint8Array(2);
  target.set(new BigInt64Array([1n, 2n]) as any);
});
