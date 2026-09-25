export const obj = {
  kind: "obj",
  twice(n: number): number {
    return n * 2;
  },
};
export const plainObj = { kind: "plain" };
export const num = 42;
export const str = "s";
export const list = [1, 2, 3];
export function fn(a: number, b: number): number {
  return a + b;
}
export const arrow = (x: number): number => x * 10;
export class Cls {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  get(): number {
    return this.v;
  }
}
export let counter = 0;
export function bump(): void {
  counter++;
}
