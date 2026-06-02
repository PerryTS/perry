// Issue #26 / #321: models effect's `SchemaAST` — a SECOND class named `Type`
// (distinct from `dup_class_name_parseresult.ts`'s `Type`) plus the
// `Type → OptionalType → PropertySignature` inheritance chain. Built into the
// importing module's instance layout, `PropertySignature` must inherit THIS
// `Type`'s fields (`type, annotations`), NOT ParseResult's `Type`.
export class Type {
  type: unknown;
  annotations: unknown;
  constructor(type: unknown, annotations: unknown = {}) {
    this.type = type;
    this.annotations = annotations;
  }
}

export class OptionalType extends Type {
  isOptional: boolean;
  constructor(type: unknown, isOptional: boolean, annotations: unknown = {}) {
    super(type, annotations);
    this.isOptional = isOptional;
  }
}

export class PropertySignature extends OptionalType {
  name: PropertyKey;
  isReadonly: boolean;
  constructor(
    name: PropertyKey,
    type: unknown,
    isOptional: boolean,
    isReadonly: boolean,
    annotations?: unknown,
  ) {
    super(type, isOptional, annotations);
    this.name = name;
    this.isReadonly = isReadonly;
  }
}
