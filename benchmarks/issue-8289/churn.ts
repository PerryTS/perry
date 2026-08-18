type Node = { v: number; w: number };

function chunk(base: number): number {
  const keep: Node[] = [];
  for (let j = 0; j < 1000; j++) {
    keep.push({ v: base + j, w: j });
  }
  let acc = 0;
  for (let j = 0; j < keep.length; j++) {
    acc += keep[j].v + keep[j].w;
  }
  return acc;
}

function main(): void {
  let total = 0;
  for (let c = 0; c < 20000; c++) {
    total += chunk(c * 1000);
  }
  console.log(total);
}

main();
