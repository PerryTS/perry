`perry-container-compose` loses the `src/commands/` tree it described as
"retained on disk as historical reference but *not* compiled": 105 lines of an
earlier `ContainerCommand` shape whose functionality now lives in
`ComposeService`'s per-method orchestration (`run_command`, `start_command`,
`build_command`, `inspect_command`) and `orchestrate::orchestrate_service`.
Git history is the reference, as the comment already pointed at.
