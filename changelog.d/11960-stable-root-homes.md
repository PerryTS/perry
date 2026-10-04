Large functions now keep GC-visible locals in one stable precise frame home
before LLVM statepoints would copy every live value around every collecting
call. A generic root-heavy-function policy switches at the measured code-size
crossover while retaining native stack maps for small and call-heavy
functions: the 200-local witness's hot function falls from 1.75 MiB to 243 KiB
and grows linearly. Moving collections still rewrite the authoritative slot
and generated code reloads it after a safepoint; no GC gate or barrier is
weakened.
