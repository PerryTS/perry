Realm header words now come from one derivation, `perry_abi::realm_header`,
fed with each birth's declared facts: the realm manifest producer and the
runtime births of realm cells call it instead of choosing GC header bits on
their own. The last 44 builtin declarations moved from `*.realm.json`
companions to their births, and the companions and their transport are gone.
Runtime births that changed: realm string arrays (`process.argv`,
`process.execArgv`, `module.globalPaths`, `navigator.languages`,
`http.METHODS`, `Module.wrapper`, `module.builtinModules`,
`tls.rootCertificates`) are born with their all-pointer layout instead of
reaching it by promotion or falling to the tag-scanned layout; empty realm
arrays (`Array.prototype`, `domain._stack`) no longer claim the raw-f64
representation; `vm.constants` is an ordinary frozen null-prototype object
as in Node; `zlib.codes` is written from one ordered table;
`module.isBuiltin` and `module.flushCompileCache` are born
non-constructable; and a runtime-internal non-enumerable key now publishes
its holder's descriptor flag.
