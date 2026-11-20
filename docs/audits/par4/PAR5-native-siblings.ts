const mutate: any = new Function("record", "mode", `
  if (mode === 0) record.target = 99;
  if (mode === 1) delete record.target;
  if (mode === 2) Object.defineProperty(record, "target", { value: 99, configurable: true });
  if (mode === 3) Object.defineProperty(record, "target", { get() { return 99; }, configurable: true });
  if (mode === 4) Object.setPrototypeOf(record, null);
`);
function privateRead(mode: number): any {
  const record = { source: 1, target: 2 };
  const sibling = { source: 3, target: 4 };
  mutate(sibling, mode);
  return record.target;
}
function publishedRead(mode: number): any {
  const record = { source: 1, target: 2 };
  mutate(record, mode);
  return record.target;
}
for (let mode = 0; mode < 5; mode++) {
  console.log("private", mode, privateRead(mode));
  console.log("published", mode, publishedRead(mode));
}
