Add a source-free installation regression for #8907, the macOS arm64
v0.5.1220 `node:http` link failure previously fixed by #5983. Exercise the
full prebuilt runtime, stdlib, and HTTP wrapper in both default and
`PERRY_NO_AUTO_OPTIMIZE=1` modes, checking that the minimal server links,
listens on an ephemeral port, closes, and exits successfully.
