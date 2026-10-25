// Helper for test_import_name_collision.ts. An unrelated class that happens to
// share the name `Shared`, with no instance methods at all. Nothing imports
// `Shared` from this module — only `helper`, which is enough to make the
// importer reach this module and register every class it exports.
export class Shared {
  static asText(value: { name: string }): string {
    return value.name;
  }
}

export function helper(): string {
  return Shared.asText({ name: "decoy" });
}
