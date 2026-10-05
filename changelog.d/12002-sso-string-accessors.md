Private field reads, class field definition, class templates, worker error
clones and `zlib` stream `off` now read string keys through the SSO-aware
accessors, so a short (inline) string key is handled safely.
