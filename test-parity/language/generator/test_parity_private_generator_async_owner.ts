function* source(start: number, end: number): Generator<number> {
  for (let i = start; i < end; i++) yield i;
}
async function consume(): Promise<void> {
  // Construct the child after the creator's native stack has returned.
  await 0;
  let total = 0;
  for (const value of source(0, 100)) total += value;
  await 0;
  console.log("async nested " + total);
}
consume();
