Builtin namespaces carry the shadow bit. Under non-constant dynamic code that
reaches the global scope, a direct namespace call (`Math.max(...)`,
`JSON.stringify(...)`, `Object.keys(...)`, `Array.isArray(...)`, `console.log(...)`)
keeps its intrinsic behind one test of its namespace's shadow bit instead of
always calling `globalThis.<ns>.<member>` generically (#12643). The runtime sets
the bit on any write, define, delete or relink of the namespace's members (the
namespace object, or a constructor's own properties) and on any change of the
`globalThis.<ns>` binding; a set bit sends the site to the generic call, which
reads the callee before the arguments. `typeof NS.member` folds the same way.
Programs without such dynamic code are unchanged.
