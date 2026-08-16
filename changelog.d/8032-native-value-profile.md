### Native values: expose the first stable `perry/native` profile (#6827)

Applications can now import fixed-width scalar markers, verified `pod<T>`
records, compile-time layout intrinsics, `PodView<T>`, and `NativeArena` from
`perry/native`. Named import aliases reuse Perry's existing verifier-backed
native layout pipeline, generated project type stubs include the module, and
`PodView.length` reports the checked record count at runtime.
