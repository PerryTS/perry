Fixed a static method extracted as a value off its class (`const f = C.m; f(...)`)
resolving to the same-named *instance* method instead of the static (#7689).
`js_class_method_bind`'s method-identity canonicalization treated a constructor
class ref like an instance receiver and consulted only the instance vtable, so
any class declaring both `static m()` and `m()` handed out the prototype method;
invoked bare, it ran with an unconstructed `this`. This broke the `marked` npm
package outright — its `Lexer.lex`/`Parser.parse` are exactly this collision, and
every `marked.parse` threw `TypeError: Cannot read properties of undefined
(reading 'pedantic')`. Constructor refs now skip the instance-vtable canonical
path and dispatch statics-first at call time; `C.prototype.m` reads are
unchanged. Covered by a runtime unit test (verified to fail pre-fix) and
`test_gap_static_method_value_name_collision.ts`.
