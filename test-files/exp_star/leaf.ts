export class Point { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } sum() { return this.x + this.y; } }
export const origin = new Point(0, 0);
export function mk(n: number) { return new Point(n, n * 2); }
export default "leaf-default";
