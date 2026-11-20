const ab = new ArrayBuffer(2);
new Uint8Array(ab).set([104, 105]);
const sab = new SharedArrayBuffer(2);
new Uint8Array(sab).set([104, 105]);
const dv = new DataView(ab);

for (const value of [ab, sab, dv]) {
  console.log(JSON.stringify([
    String(value),
    `${value}`,
    value.toString(),
    (value as any).toString("hex"),
  ]));
}

const buffer = Buffer.from("hi");
console.log(JSON.stringify([buffer.toString(), buffer.toString("hex")]));
