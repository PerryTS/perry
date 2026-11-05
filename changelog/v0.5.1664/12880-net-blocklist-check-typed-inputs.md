### Fixed

- Correct `net.BlockList.check` typed-input admission before string/JSON coercion: preserve address-before-family errors, accept actual `SocketAddress` inputs without inspecting the family argument, and return `false` for invalid textual address/family combinations. Legacy check forwarding reuses the rooted native-method path.
- Add a 42-row public node-suite regression fixture covering direct, saved, computed, erased and prototype calls without coercing poison inputs. Historical matched-world writer and independent verification each recorded 22 safe-case passes, 22 remaining parity failures and one unchanged native timeout, with three unsafe cases not run. NET remains incomplete; distinct legacy HANDLE_TAG branch execution is not dynamically proven.
