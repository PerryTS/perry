Fix Proxy values in `Array.from` and dynamic `RequestInit.headers` (#10270,
#10274; OpenCode bring-up tracker #10107).

- Route array proxies through iterator materialization before inspecting heap
  headers, including mapped `Array.from`, Headers iterable pairs, call spread,
  and collection constructors. Preserve the proxy receiver when a get trap
  forwards the default iterator, and safely dispatch array-like proxy indices.
  Keep concat on trapped indexed reads, preserving
  holes and ignoring a custom iterator.
- Convert dynamic Request header records/iterables with
  `js_headers_init_from_value`; retain the existing inline literal-header path.
- Keep proxy registry lookups behind the existing proxy-id band predicate;
  Array.from checks only within its existing small-handle branch.
- Add compiled regression probes for both issue reproductions, nested proxies,
  get/ownKeys traps, custom iterators, mapped conversion, and sibling consumers.

Read a Proxy source’s iterator method once before constructing the result, and
cache the iterator’s next method. Keep the method, iterator, intermediate values,
and pending mapping errors rooted across user callbacks and iterator closing.

Pass an existing Headers handle straight to the Request constructor, which
already clones its entries, instead of copying it into a temporary Headers
store first. Records, iterables, Proxies, `undefined` and `null` still take the
full conversion path.
