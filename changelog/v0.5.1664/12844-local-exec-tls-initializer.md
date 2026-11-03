`with_local_exec_tls` searched each global's whole line for
`thread_local `, initializer included. A string image whose bytes spell the
word (highlight.js's C++ keyword list, bundled by Claude Code 2.1.112) was
rewritten to `thread_local(localexec)`, 11 bytes longer than its declared
`[839 x i8]`, and the module failed to parse: "constant expression type
mismatch". The rewrite now reads the space-separated keywords before
`global`/`constant` only.
