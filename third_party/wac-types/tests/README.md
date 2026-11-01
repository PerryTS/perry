The `dummy_wasi_http@*.wasm`'s here were generated via the follow:
```console
$ wkg get wasi:http@* --format wasm 
$ wasm-tools component wit wit/wasi_http@*.wasm -o wasi_http@*.wit 
$ wasm-tools component embed --dummy wasi_http@*.wit | wasm-tools component new - -o dummy_wasi_http@*.wasm
```