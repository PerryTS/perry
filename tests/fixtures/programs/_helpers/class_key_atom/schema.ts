// The zod shape: a field that aliases a method, then the constructor binds
// both through `this`. `alias` is spelled in this module only, so its image
// literal is this module's, while the importer builds the class key lists.
export abstract class Schema {
  kind: string;
  alias = this.check;
  constructor(kind: string) {
    this.kind = kind;
    this.check = this.check.bind(this);
    this.alias = this.alias.bind(this);
  }
  check(value: unknown): string {
    return this.kind + ":" + typeof value;
  }
}

export class StringSchema extends Schema {
  static create = (): StringSchema => new StringSchema("string");
}
