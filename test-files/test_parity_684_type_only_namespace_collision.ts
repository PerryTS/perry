import { kindOf } from "./fixtures/parity_5891/type_only_namespace/Data.ts";
import type * as Schema from "./fixtures/parity_5891/type_only_namespace/Schema.ts";

console.log(kindOf(42));
console.log(kindOf("hi"));
type _Unused = ReturnType<typeof Schema.kindOf>;
