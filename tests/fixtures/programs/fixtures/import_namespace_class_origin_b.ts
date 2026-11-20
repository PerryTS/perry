export class Service {
  kind() { return "b"; }
  static label() { return "b"; }
  static *[Symbol.iterator]() { yield "b"; }
}
export * as B from "./import_namespace_class_origin_b.ts";
