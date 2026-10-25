Fixed the `js_ext_net_socket_on` declarations in `perry-stdlib` and codegen, which said `void` while the definition returns the handle (`i64`); `wasm-ld` rejects the mismatch without LTO.
