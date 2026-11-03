### Fixed

- Enforce Node Worker restrictions before process-wide operations, expose disabled callable metadata and hide unavailable process internals without adding another worker identity flag.
- Keep computed Worker title assignments live through the existing title provider while leaving existing named-import behavior unchanged and avoiding Linux process-name updates from workers.
