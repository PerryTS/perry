### Fixed

- BroadcastChannel core methods and its read-only name getter now use the actual receiver, including borrowed methods, transparent proxies and objects derived from genuine channels, while preserving validation and clone ordering.
