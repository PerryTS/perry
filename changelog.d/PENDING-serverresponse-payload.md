### Fix standalone ServerResponse retention during fastify inject

Standalone `http.ServerResponse` instances now own a typed native payload through their traced `native_state` word. Their sockets and listeners live in ordinary object-owned JS state, so response/listener/socket cycles are collectible. The constructor no longer registers a global handle or installs the HTTP handle root scanner. End and destroy release native state explicitly; sweep remains the backstop. Observable terminal properties survive explicit release.

Native-this aliases now use one traced forwarding record for both handles and payload objects. An object that is also a stream keeps that record in the stream record's `NativeAlias` slot; neither family overwrites the other's state. Transport responses and bare OutgoingMessage handles retain their existing transport ownership.

Coverage includes captured-response listener/socket cycles, explicit release through every end entry point, global-root sabotage, and both alias/stream construction orders. The compiled alias lifetime test and the reproducible 20,000-response benchmark include once(close), once(error), and socket error listeners capturing the response. The handle ledger loses the standalone producer; its already-stale weakref/index entry is also removed to make the ledger match the tree.
