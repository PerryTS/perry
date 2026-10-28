import { helper } from "./fixtures/import_namespace_class_origin_a.ts";
import { A } from "./fixtures/import_namespace_class_origin_a.ts";
import { B } from "./fixtures/import_namespace_class_origin_b.ts";
import { Service } from "./fixtures/import_namespace_class_origin_b.ts";

const a = new A.Service();
const b = new B.Service();
const bare = new Service();
console.log(A.Service.label(), B.Service.label(), Service.label());
console.log(a.kind(), b.kind(), bare.kind(), helper().kind());
console.log(a instanceof A.Service, a instanceof B.Service);
console.log(b instanceof B.Service, b instanceof A.Service, b instanceof Service);
console.log(bare instanceof Service, bare instanceof B.Service, bare instanceof A.Service);
function* values() {
  yield* A.Service;
  yield* B.Service;
}
console.log(Array.from(values()).join(","));
