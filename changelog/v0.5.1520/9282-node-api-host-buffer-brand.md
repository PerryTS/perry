Fixed the feature-gated Node-API host build after the Buffer branding refactor.
Allowlisted native addons once again link the complete 145-symbol host ABI and
load their authenticated sidecars instead of falling back to a runtime without
Node-API support.
