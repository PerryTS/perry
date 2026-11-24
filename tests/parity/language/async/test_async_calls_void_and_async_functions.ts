// An async body calls a function with no return value, and another async function
// with and without its optional argument.
function note(label: string, value: unknown) {
  console.log(label, JSON.stringify(value));
}
async function inner(x: number, y?: number): Promise<number> {
  return x + (y ?? 10);
}
async function outer(): Promise<string> {
  note("note", [1, 2]);
  const a = await inner(1);
  const b = await inner(1, 2);
  return a + " " + b;
}
outer().then((v) => console.log(v), (e) => console.log("threw " + e.message));
