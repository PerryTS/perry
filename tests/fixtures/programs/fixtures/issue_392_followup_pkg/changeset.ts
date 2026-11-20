export class Changeset {
  readonly adds = new Map<number, unknown>();
  readonly removes = new Set<number>();

  set(componentType: number, component: unknown): void {
    this.adds.set(componentType, component);
    this.removes.delete(componentType);
  }

  delete(componentType: number): void {
    this.removes.add(componentType);
    this.adds.delete(componentType);
  }
}
