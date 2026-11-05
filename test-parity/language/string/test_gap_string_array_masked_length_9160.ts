const strings: string[] = ["a", "abcdefghijklmnop", "猫", ""];

function sumLengths(): number {
  let total = 0;
  for (let i = 0; i < 12; i++) {
    total += strings[i & 3].length;
  }
  return total;
}

console.log(sumLengths());

// TypeScript annotations are erased. The optimized clone must reject this
// window and preserve ordinary `.length` / `+` behavior in the slow copy.
const lied: string[] = ["ok", "still a string"];
(lied as any)[1] = 42;

function sumLiedLengths(): number {
  let total = 0;
  for (let i = 0; i < 4; i++) {
    total += lied[i & 1].length;
  }
  return total;
}

console.log(sumLiedLengths());

function liedAccumulator(): number {
  let total: number = "x" as any;
  for (let i = 0; i < 4; i++) {
    total += strings[i & 3].length;
  }
  return total;
}

console.log(liedAccumulator());
