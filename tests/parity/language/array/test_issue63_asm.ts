// Minimal escape: just create and store. Check disassembly.
export function makeThree(i: number): number[] {
  return [i, i + 1, i + 2];
}

export function makeFour(i: number): number[] {
  return [i, i + 1, i + 2, i + 3];
}

// Tiny driver so the functions aren't dead-eliminated
const sink: number[][] = [];
for (let i = 0; i < 10; i++) sink.push(makeThree(i), makeFour(i));
console.log(sink.length);
