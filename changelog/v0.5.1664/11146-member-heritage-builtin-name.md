fix(hir): `class X extends ns.URL {}` no longer constructs Perry's native URL (#11139)

A member-expression heritage kept only its trailing property as the class's
`extends_name`, and codegen keys its built-in `super()` routes on that bare
name. When 7df369070 (#10639) added `URL` to those routes, every package class
reached as `something.URL` was built as a native URL instead of running its own
constructor. mongodb-connection-string-url declares
`class URLWithoutHost extends whatwg_url_1.URL {}`, so whatwg-url's brand check
rejected every `ConnectionString`, and `new MongoClient(uri)` threw
`'get protocol' called on an object that is not a valid instance of URL.`
The same misroute applied to any other name in that list (`ns.Map`, `ns.Date`,
typed arrays, and so on).

Both class lowering paths (`lower_class_decl` and `lower_class_from_ast`) now
drop the static name and parent link for a member heritage whose trailing name
is one of those built-ins, so the parent resolves through `extends_expr`. The
exceptions are an object that is a global-object alias (`globalThis.URL`) or a
native-module binding (`url.URL` from `import * as url from "url"`), which
still mean the built-in.

Tests: `test_gap_11139_member_heritage_builtin_name.ts` (the vendored
whatwg-url / mongodb-connection-string-url module shapes under
`test-files/fixtures/issue_11139_member_heritage/`) and
`perry-hir` `lower::tests::issue_11139_member_heritage_builtin_name`.
