export class MatrixLike {
  label: string;
  constructor(label: string) { this.label = label; }
  decompose() { return "decompose:" + this.label; }
}
export class BaseObject {
  constructor() {
    Object.defineProperties(this, { hiddenMarker: { value: "hidden", configurable: true } });
    (this as any).plainBase = "base";
    (this as any).matrixWorld = new MatrixLike("world");
  }
  updateMatrixWorld(force?: boolean) { return (this as any).matrixWorld.decompose() + ":" + String(force); }
}
