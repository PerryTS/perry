### Fixes

- Enforce the Fetch standard's blocked destination ports before connection creation or reuse, including followed redirects. Rejections match Node's `TypeError: fetch failed` with `cause.message === 'bad port'` and no `cause.code`. Manual and error redirect modes retain their existing behavior. Includes all 83 standard ports; port 0 follows the standard even though Node 26.5.1 currently permits it past preflight.
