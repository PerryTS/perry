CI: the component-model unit tests run in their own `wasi-component-units` job, so `wasi-native` keeps only its codegen and link-argument tests and no longer sits on the core-PR critical path.
