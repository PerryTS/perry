Fix the no-default-features product build under `-D warnings` by compiling
RegExp-only GC barrier and prototype-site state only for tests or when the
`regex-engine` feature is enabled.
