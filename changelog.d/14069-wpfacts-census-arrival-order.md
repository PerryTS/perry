The whole-program facts birth census (`PERRY_FACTS_BIRTH_CENSUS=1`) now
stamps one clock over the events that cause later ones: each node's first
unknown value (`birth-arrival`: copy source or injection, with the gate it
replays), each value's first escape (`seq` on `escape-first`) and each gate
opener. `scripts/top_births/chain.py` follows causes back by strictly earlier
events, so a chain ends at its seed instead of looping. Diagnostic only:
compilation without the census is unchanged.
