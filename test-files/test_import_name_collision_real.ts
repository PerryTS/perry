// Helper for test_import_name_collision.ts. The class the consumer actually
// imports: an ordinary instance method, reached through a constructor callback
// so the call cannot be resolved statically.
export class Shared {
  constructor(block?: (s: Shared) => void) {
    if (block !== undefined) block(this);
  }

  defineThing(name: string): string {
    return `thing:${name}`;
  }
}
