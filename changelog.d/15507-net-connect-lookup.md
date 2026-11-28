- **`net.connect` `lookup` option.** `net.connect({ lookup })` ignored the
  caller's resolver and used the system one, so names only the custom lookup
  knew failed with `ENOTFOUND`. Connect now calls `lookup` with Node's dns
  options, reports each answer through `'lookup'` events, rejects invalid
  answers with `ERR_INVALID_IP_ADDRESS` / `ERR_INVALID_ADDRESS_FAMILY`, and
  connects to the returned addresses, trying the next when one is refused.
