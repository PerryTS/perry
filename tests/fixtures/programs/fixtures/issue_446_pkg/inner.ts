export class Inner {
  readonly adds = new Map<number, string>();

  setAdd(k: number, v: string): void {
    this.adds.set(k, v);
  }
}
