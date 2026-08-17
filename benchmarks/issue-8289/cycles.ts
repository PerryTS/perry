class Cell {
  peer: Cell | null;
  payload: number;

  constructor(payload: number) {
    this.peer = null;
    this.payload = payload;
  }
}

function makeCycle(i: number): number {
  const a = new Cell(i);
  const b = new Cell(i + 1);
  a.peer = b;
  b.peer = a;
  return a.payload + (b.peer as Cell).payload;
}

function main(): void {
  let acc = 0;
  for (let i = 0; i < 2_000_000; i++) {
    acc += makeCycle(i);
  }
  console.log(acc);
}

main();
