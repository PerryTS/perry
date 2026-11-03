// A namespace member is a class only when the namespace's module exports a
// class under that name. A class with the same name imported from another
// module says nothing about it. opencode v2.1.2: the skill plugin imports
// `Document` from the config schema, whose module also declares `class Info`,
// and `Skill.Info.make({...})` (a schema value) was lowered as a static call
// on that class. Its miss path returned the receiver, so the plugin added the
// schema itself and failed with SchemaError(Expected Skill.Info).
import { Document } from "./_helpers/namespace_member_class_name/classes.ts";
import * as Named from "./_helpers/namespace_member_class_name/classes.ts";
import { Values } from "./_helpers/namespace_member_class_name/values.ts";
import * as Direct from "./_helpers/namespace_member_class_name/values.ts";

const show = (label: string, v: unknown) =>
  console.log(label, typeof v, v === Values.Info || v === Values.Name, JSON.stringify(v));

show("Values.Info.make", Values.Info.make({ a: 1 }));
show("Direct.Info.make", Direct.Info.make({ b: 2 }));
show("Values.Name.make", Values.Name.make({ c: 3 }));
show("Direct.Name.make", Direct.Name.make({ d: 4 }));
console.log("typeof Values.Shape", typeof Values.Shape, typeof Direct.Shape);
console.log("Direct.Shape.kind", Direct.Shape.kind);

// Real namespace classes keep their static dispatch.
console.log(Named.Info.make({ e: 5 }), Named.Name.make({ f: 6 }), Named.Shape.tag);
console.log(typeof Named.Info, typeof Named.Shape, Document.make(7));
console.log(Direct.Klass.make(8), Direct.Klass.inherited(), Values.Klass.make(9), typeof Direct.Klass);
