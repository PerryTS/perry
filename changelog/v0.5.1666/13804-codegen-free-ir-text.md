The in-process LLVM IR parse now disposes the copy of the IR text it parsed instead of leaking it, which removes a leak of about 40 MB per codegen unit on large programs.
