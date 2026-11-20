Whole-program facts no longer model `export type T = ...` or `export interface I`
(or `export { I }` of a local interface) as a module export whose value is
unknown. Such an export is erased at run time, so the module namespace has no
key for it; the facts now ask the same HIR predicate as codegen and the
namespace builders (`Module::is_type_only_export_binding`), so a computed read
of a namespace that lists type exports (`(ns as any)[k]`) no longer holds an
unknown value and its receivers' sites can be proven.
