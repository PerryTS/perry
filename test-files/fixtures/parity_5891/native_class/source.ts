export class SourceLike {
  version = 0;
  set needsUpdate(value: boolean) { if (value === true) this.version++; }
}
