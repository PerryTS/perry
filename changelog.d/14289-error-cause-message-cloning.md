### Fixed

- Preserve own data Error causes, including undefined values and shared/cyclic references, during message cloning; capture the cause before reentrant stack getters can change it.
