fix(compile): a class reached only through an imported class's member types no longer shadows a global (#10356).

Second registration path for #10356. The first fix guarded the implicit import-walk loop, which registers every exported class of a module the importer touches. That loop is not the only way a class the importer never named gets bound under its bare name: the transitive class closure in `run_pipeline` also registers whatever an imported class's FIELD and RETURN types mention.

OpenCode's generated SDK is exactly this shape:

```ts
export class Request extends HeyApiClient { ... }
export class OpencodeClient {
  private _request?: Request      // field type
  get request(): Request { ... }  // getter return type
}
```

so `import { OpencodeClient }` registered `Request`, and `new Request(url, init)` in the importer built the SDK's class instead of the global. The TUI died on `next.headers.delete(...)` with "Cannot read properties of undefined". This is why the first fix passed every synthetic probe and still left OpenCode broken — the probes' `OpencodeClient` had no member typed `Request`.

Fix: the closure walk skips a ref whose name is a global intrinsic value name, the same predicate the import-walk guard uses.

Parent refs stay exempt: `class Sub extends Request` genuinely needs its parent's layout registered or instances allocate too few inline slots (#485), and parent refs already resolve path-aware in the child's own module (#26/#321). That leaves a known remaining gap — importing a subclass of a global-named class still binds the parent's name in the importer — which is tracked separately and deliberately not asserted by the test.

Validation: `crates/perry/tests/issue_10356_closure_walk_shadows_global.rs` pins the two-module case byte-for-byte against bun 1.3.14, including the positive direction (the SDK's own member typed `Request` must still resolve to the SDK class).
