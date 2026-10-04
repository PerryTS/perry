**Node-compatible received-value diagnostics.**

Shared argument errors now render numbers and truncated UTF-16 strings like
Node.js, preserving lone surrogate code units in JavaScript messages. The
runtime also exposes a rooted string-valued diagnostic ABI, and JSON quoting
validates WTF-8 bytes before creating a Rust string reference.
