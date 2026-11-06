let cb: any = () => {};
export function setCallback(f: any): void { cb = f; }
export class CallbackBase {
  scale: number;
  constructor(k: number) { this.scale = k; }
  step(x: number): number { cb(); return (x & 255) * this.scale + 7; }
}
export class CallbackDerived extends CallbackBase {
  step(x: number): number { cb(); return (x & 255) * this.scale + 11; }
}
