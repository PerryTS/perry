fix(codegen): a namespace member is a class only when its namespace exports one

`ns.Member.method(...)` was lowered as a static call on a class whenever any
class named `Member` was known in the module, for example a class from an
unrelated import. The namespace-scoped class identity
(`namespace_member_class_key`) was already recorded, but the static-dispatch
receiver check and `typeof ns.Member` used the bare member name.

opencode v2.1.2: the skill plugin imports `Document` from the config schema,
whose module also declares `class Info`. `Skill.Info.make({...})` (a schema
value) became a static call on that class, the miss path returned the
receiver, and the plugin added the schema itself and failed with
`SchemaError(Expected Skill.Info)`, shown as "1 plugin failed".

The receiver is a class only when the namespace-scoped identity and the bare
name register the same class; anything else keeps the generic call. Gap test
`test_gap_namespace_member_class_name` covers an inherited static, a declared
static, `typeof`, and real namespace classes, including re-exported ones.
