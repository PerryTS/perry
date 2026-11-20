// Mixed generated/runtime bumps, birth shapes, forwarding aliases and polls.
// Run with Node --expose-gc and Perry's force/verify evacuation instruments.
declare function gc(): void;
class Cell {
  value: number;
  text: string;
  constructor(value: number) { this.value = value; this.text = "cell" + value; }
}
const proto = { inherited: 73 };
const floorProto = { inherited: 19 };
let checksum = 0;
for (let epoch = 0; epoch < 24; epoch++) {
  const values: any[] = [];
  const alias = values;
  const floorValues: any[] = [];
  for (let i = 0; i < 1400; i++) {
    const object = Object.create(proto);
    object.value = i;
    object.cell = new Cell(i);
    object.text = "odd-" + epoch + "-" + i;
    values.push(object);
    if (i % 7 === 0) {
      const floor = Object.create(floorProto);
      floorValues.push(floor);
    }
    if (i % 350 === 349) gc();
  }
  gc();
  for (let i = 0; i < floorValues.length; i++) floorValues[i].value = i;
  gc();
  for (let i = 0; i < floorValues.length; i++) {
    if (floorValues[i].value !== i || floorValues[i].inherited !== 19) throw new Error("floor");
    if (Object.keys(floorValues[i]).length !== 1) throw new Error("hidden floor");
  }
  for (let i = 0; i < alias.length; i++) {
    checksum += alias[i].value + alias[i].cell.value + alias[i].inherited;
    if (Object.getPrototypeOf(alias[i]) !== proto) throw new Error("prototype");
    if (alias[i].cell.text !== "cell" + i) throw new Error("cell");
    if (alias[i].text !== "odd-" + epoch + "-" + i) throw new Error("text");
  }
}
console.log(checksum);
