// Issue #636: namespace-import resolved member call dispatched receiver
// as TAG_TRUE → "(boolean).fn is not a function". v0.5.730 adds:
//   1. CLI: populate imported_vars for namespace exports that are vars
//      (mirror the named-import path).
//   2. Codegen: early arm in lower_call detecting
//      Call { callee: PropertyGet { ExternFuncRef(ns), method } }
//      where ns is in namespace_imports — routes to zero-arg getter +
//      closure call (var-shaped) or direct fn call (decl-shaped).
import * as inner from "./test_issue_636_inner.ts";
const a = inner.make("hello");
console.log("var-shaped:", a);
const b = inner.decl(7);
console.log("decl-shaped:", b);
