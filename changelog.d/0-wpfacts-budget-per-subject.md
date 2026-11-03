The whole-program facts solve never gives up on a whole program any more.
Each function body (and each shared object such as a builtin prototype) has
its own work budget; one that goes over it is compiled as unseen code: its
parameters, results and the objects it touches are unknown, its own sites
stay unproven with the cause `gave-up-budget`, and every other function keeps
its facts. Each give-up is reported once on stderr (`perry: wpfacts gave-up
<function> work=<w> budget=<B> kind=<work|pts|nodes>`) and in the facts dump.
The wall-clock budget (`PERRY_FACTS_BUDGET_SECS`) is gone: the budget counts
solver work, so the result is the same on every machine.
