export class Changeset {
  readonly adds = new Map<number, unknown>();

  set(componentType: number, component: unknown): void {
    this.adds.set(componentType, component);
  }
}
