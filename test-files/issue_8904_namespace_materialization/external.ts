export interface SchemaShape {
  readonly kind: string;
}

export type SchemaInput = Record<string, unknown>;

function _object(): string {
  return "object";
}

export { _object as object };

export * as coerce from "./coerce.ts";
