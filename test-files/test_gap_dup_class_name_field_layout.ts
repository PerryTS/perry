// Issue #26 / #321: cross-module same-named-class field-layout pollution.
//
// effect's `Schema.ts` imports BOTH `ParseResult.Type` (fields
// `_tag, ast, actual, message`) and `SchemaAST`'s `PropertySignature`
// (← `OptionalType` ← a DIFFERENT `Type`, fields `type, annotations`), then
// constructs `new PropertySignature(...)`. Perry's class registry / keys-array
// builder and constructor field-initializer chain walk resolved the parent
// `Type` by BARE NAME against a name-keyed table that holds only ONE same-named
// stub — so `PropertySignature` instances inherited ParseResult.Type's four
// fields as spurious `undefined` slots. `Object.keys()` then reported nine keys
// (the union) instead of five, corrupting effect's schema AST so decode/encode/
// is read `_tag` of `undefined`.
//
// Fix: disambiguate same-named parent classes by source module — a class's
// `extends` resolves in its OWN module's scope. The transitive-import closure
// (compile.rs) now imports the correct-module parent even past the bare-name
// dedup, and codegen resolves the parent chain (keys-array, allocation field
// count, AND constructor field-init) by preferring the parent stub whose source
// prefix matches the child's. Compared byte-for-byte against
// `node --experimental-strip-types`.

import { Type as PRType } from "./_helpers/dup_class_name_parseresult.ts";
import { PropertySignature } from "./_helpers/dup_class_name_schemaast.ts";

// PRType imported FIRST so its `Type` wins the bare-name race in the importing
// module's stub table — the order that triggered the original pollution.
const pr = new PRType({ kind: "ast" }, 5, "msg");
console.log("PRType._tag:", pr._tag);

const ps: any = new PropertySignature(
  "name",
  { _tag: "StringKeyword" },
  false,
  true,
  {},
);

// Must be exactly the five OptionalType/Type/PropertySignature fields — NOT
// the polluting `_tag, ast, actual, message` from the OTHER module's `Type`.
console.log("keys:", Object.keys(ps).sort().join(","));
console.log("name:", ps.name);
console.log("isReadonly:", ps.isReadonly);
console.log("isOptional:", ps.isOptional);
console.log("type._tag:", ps.type._tag);
console.log("has _tag:", "_tag" in ps);
console.log("has ast:", "ast" in ps);
