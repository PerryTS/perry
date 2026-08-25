### Fixed

- Completed Node `async_hooks` lifecycle support across async resources, event emitters, HTTP, sockets, workers, zlib, DNS, and WebCrypto. Provider scopes now restore execution and `AsyncLocalStorage` state when hooks or callbacks throw, deferred destroy hooks run at the correct lifecycle boundary, and allocation-sensitive values remain rooted across moving garbage collections.
