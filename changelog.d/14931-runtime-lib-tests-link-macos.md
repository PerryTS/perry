`cargo test -p perry-runtime --lib` links on macOS again. The execution-prefix
image writer and one of its tests named the ELF linker symbols `__data_start`,
`_end`, `__executable_start` and `__start_/__stop_perry_{strings,layouts,atoms}`
on every target; they are now declared on Linux only. Elsewhere, where no image
is ever captured, the writer sees empty section and executable ranges.
