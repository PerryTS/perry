On macOS, `perry compile --debug-symbols` keeps debug info for modules split into codegen units: the per-unit objects stay on disk until `dsymutil` has read them (#15238).
