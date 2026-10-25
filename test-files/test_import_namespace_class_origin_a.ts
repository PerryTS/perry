export class Service {
  kind() { return "a"; }
  static label() { return "a"; }
  static *[Symbol.iterator]() { yield "a"; }
}
export function helper() { return new Service(); }
export * as A from "./test_import_namespace_class_origin_a.ts";
