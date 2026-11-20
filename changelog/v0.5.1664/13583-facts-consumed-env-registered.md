Register `PERRY_FACTS_CONSUMED` (the codegen-consumed marks of the facts
dump) with the compile environment as a diagnostics-only variable, like
`PERRY_FACTS_DUMP`: it records which sites read a program fact and changes
no emitted code, so it is not a build-cache input. Fixes the environment
audit test on main.
