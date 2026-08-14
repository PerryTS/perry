### Fixed

- **`proxy_locals` is keyed on the resolved binding, not the spelling (#7775).** If any
  function anywhere in a module bound a name to `new Proxy(...)`, every function using that
  spelling had its property reads, property writes and `in` checks rewritten to proxy
  operations — on receivers that are not proxies. `js_proxy_get` on a non-proxy returns
  `undefined` rather than throwing, so the wrong answer was silent: a read came back
  `undefined`, a `for (i = 0; i < a.length; i++)` after it ran zero iterations, and nothing
  in the build or the run said so. The proxy's own function never had to be called; only the
  identifier had to match.

  Ten lines were enough to reproduce it. Perry printed `undefined`, node printed `42`:

  ```ts
  function neverCalled(): number {
    const raw = { v: 1 };
    const a: any = new Proxy(raw, {});
    return a.v;
  }
  function readObj(): number {
    const a = { v: 42 };
    return a.v;
  }
  console.log(readObj());
  ```

  Renaming the proxy's local to `q` made Perry print `42`. The trigger was the spelling and
  nothing else.

<details>
<summary>Root cause, the shape of the repair, and what it deliberately does not cover</summary>

**Root cause.** `LoweringContext::proxy_locals` was a bare-name `HashSet<String>` populated
by a module-wide pre-scan (`lower/pre_scan/weakref_locals.rs`) that runs before bindings are
resolved. Seven consumers asked it `contains(&name)` — property read
(`lower/expr_member.rs`), property write and compound write (`lower/expr_assign.rs`),
`in` (`lower/lower_expr/arm_bin.rs`), `new p(...)` (`lower/expr_new.rs`), `p(...)`
(`lower/expr_call/post_args_dispatch.rs`) and the array-method fold guard
(`lower/expr_call/local_array_methods.rs`) — so the answer could not depend on which binding
the receiver actually was.

An earlier pass at this added one arm to the pre-scan's "poison" set, subtracting names whose
initializer was a call or an `await`. Its own comment admitted the limit, and the limit was
the larger half: `record_var` only matches `ast::Pat::Ident`, so a function parameter, a
`for...of` head, a destructuring pattern and a catch param could never be poisoned whatever
they held; and a literal, a member read or an identifier copy is not a call, so those escaped
too.

**The repair.** `proxy_locals` keeps its name, and stops being authoritative. A new
`proxy_local_ids: HashSet<LocalId>` records the *resolved bindings* that actually hold a
proxy, and every consumer now asks `LoweringContext::is_proxy_local(name)`, which resolves
the receiver first:

```rust
match self.lookup_local(name) {
    Some(id) => self.proxy_local_ids.contains(&id),
    None => self.proxy_locals.contains(name),
}
```

Registration happens in three places, all at lowering time where locals are resolved:

| site | covers |
|---|---|
| `destructuring/var_decl/alias_tracking.rs` (`track_decl_aliases`) | every `let`/`const`/`var` declarator whose lowered init is `Expr::ProxyNew` |
| `lower/stmt.rs` (`Proxy.revocable` destructuring) | `const { proxy, revoke } = Proxy.revocable(...)` |
| `lower/lower_module_fn.rs` (module-var pre-registration) | a module-level proxy read from a function body lowered *before* its declarator |

Keying registration off the *lowered* init rather than the AST means it inherits
`lower_new_expr`'s `shadowed_by_user_binding` check for free: a user `class Proxy {}` never
lowers to `Expr::ProxyNew`, so it never registers (#6233's rule still holds). The
module-level pre-registration arm is the one exception — it runs before any declarator is
lowered, so it matches the AST (`decl_init_is_proxy_new`) and gates on the pre-scan's own
`proxy_locals` membership, which already applied the shadow rule.

**It also closes the same bug pointing the other way.** The pre-scan's `walk_stmt` descends
into function *declarations* only, never into a class body or an arrow body, so a proxy
declared in a method was never registered at all and its traps never took the fast path. That
was observable: a `set` trap reached through the generic dynamic write path read back
`undefined` instead of the trapped value. Registering at the declarator, wherever the
declarator sits, fixes it — and the new coverage asserts both directions in one module, since
a fix that widened registration while re-breaking the narrowing would still look green on
either half alone.

**Known hole, stated plainly.** A receiver that resolves to *no* local — a bare global, or a
binding reached through a lowering path that does not pre-register it — still falls back to
the name set and is still scope-blind. That arm is kept deliberately: dropping it would
regress genuine proxies reached through those paths, and it is strictly no worse than the
previous behaviour, which used it for everything. The pre-scan's poison set also stays, now
as a second line of defence that narrows only that fallback. This is written into the doc
comment on `is_proxy_local` so the next reader does not mistake the repair for total.

**Coverage.** `crates/perry-hir/src/lower/proxy_binding_tests.rs` (4 tests, `cargo-test`-visible
per #5960) pins *which lowering* each receiver got, per function — an output test would not do,
because the failure mode is a plausible-looking `undefined`. Each test carries a positive
control in the same module (a receiver that must still route to `ProxyGet`), so a test cannot
pass by the feature having switched off everywhere. All four were watched fail: three against
an `is_proxy_local` reverted to name-only semantics, and the forward-reference one against the
module-var pre-registration arm disabled.

`test-files/test_gap_proxy_local_name_collision_7775.ts` grew the eight behavioural shapes,
byte-compared against node: an object literal, a function parameter, `.length` after `.push()`
on `const items: number[] = []`, an identifier copy, a `for...of` head, a destructured binding,
a property write plus compound write, and `"v" in t` — every one of them `undefined`/`NaN`/
`false` before, correct after — plus a proxy declared in a class method and in an arrow body,
and a forward-referenced module-level proxy.

**Validation.** All 14 `.ts` probes byte-match node 26.5.1 (8 of them, plus a compound-assign
case, were wrong before). All 18 compilable `test-files/*` that mention `new Proxy` byte-match
node; the 19th is a `.tsx` the CLI does not take as a bare positional, unrelated to this change.
`cargo test -p perry-hir` is green (515 passed, 0 failed across its targets).
`cargo test -p perry-codegen`'s two `loop_safepoint_purity` failures were confirmed
pre-existing by A/B against a pristine tree.

</details>
