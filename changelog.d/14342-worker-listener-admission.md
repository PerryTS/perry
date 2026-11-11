### Fixed
- Preserve original Worker listener arguments and share its add/remove aliases with the existing listener registry; validate MessagePort listeners and emit deferred nullish-listener warnings without rejecting object listeners.
