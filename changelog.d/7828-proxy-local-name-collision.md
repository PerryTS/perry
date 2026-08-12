**A local that merely shares a name with a proxy local is no longer treated as a proxy.**
`proxy_locals` is a bare-name set collected by a module-wide pre-scan with no
scope discrimination, and `expr_member` lowered `<name>.prop` to
`Expr::ProxyGet` for any function using that name. `js_proxy_get` on a
non-proxy answers `undefined`, so a plain array's `a.length` came back
undefined and the `for (i = 0; i < a.length; i++)` after it ran **zero
iterations** — with no diagnostic, and without the proxy's own function ever
being called, without the proxy being over the same array, and without it being
an array at all. Only the name had to collide. Two defects in the `poison`-set
remedy the pre-scan already documents for this hazard: it fired only for a
colliding `new <OtherClass>()`, so a name bound to a *call*
(`const a = build(10)`) poisoned nothing; and its result was subtracted from
`weakmap_locals` and `weakset_locals` only — two of the five sets it feeds — so
even the covered case left `proxy_locals` wrong, making a colliding
`new Other()` instance read `a.v` as `undefined`. The comment restricting the
subtraction weighed a lost codegen fast path against "no upside", reasoning
about *method* dispatch; for a *property* read the fallback is correct and what
a poisoned name buys back is a wrong answer. Verified by fixture rather than
argument: a genuine, actually-used proxy under a poisoned name still returns 42
from its `get` trap and its `has` trap still works.
`test-files/test_gap_proxy_local_name_collision_7775.ts` asserts both
directions, since a fix that broke proxies instead would otherwise pass. (#7775)
