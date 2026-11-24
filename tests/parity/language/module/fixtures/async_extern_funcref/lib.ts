export function add(a: number, b: number) { return a + b; }
export function inc(x: number) { return x + 1; }
export async function dbl(x: number) { return x * 2; }
export function getAdd() { return add; }
export function isAdd(f: unknown) { return f === add; }
