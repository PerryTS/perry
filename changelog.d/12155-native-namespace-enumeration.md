### Fix native CommonJS export enumeration

- Populate zlib's enumerable export surface with the pinned Node 26.5.1 key set, including compression constructors, factories, one-shot functions, `constants`, and `codes`. The previous table contained only `codes`, so import-star helpers and object spread lost `Gunzip` even though direct reads worked.
- Copy callable sources through own-property descriptors and `[[Get]]`, preserving the `events` CommonJS export's lazy accessor properties and getter-driven changes to later keys.
- Keep the `NativeNamespace` shape gate intact. The shared creator already publishes that kind at birth; the zlib table was incomplete before #12010 as well.
- Add Node parity coverage for keys, for-in, entries, import-star copies, and spread across zlib, fs, path, events, and crypto, plus constructor lookup, createRequire, builtin aliases, and callable accessors.
