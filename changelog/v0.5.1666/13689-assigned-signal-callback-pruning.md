Fix unnecessary IPC and regex retention when a discarded process signal listener
registration assigns a lexical arrow to a local variable. Preserve the callback's
identity for `removeListener` and `off` without adding analysis tables or caches.
