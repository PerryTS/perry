**The A/B now proves turnloop did the I/O, not just that it did the waiting.**

`scripts/turnloop/server_ab.py` verified each arm by its marker line — the
turnloop arm must print `[perry-loop] driver=turnloop`, the baseline must not.
That proves the **wait driver**, which is not the same claim as "turnloop served
these requests", and the gap is the shape CLAUDE.md warns about: a gate that runs
while its subject never did.

`try_listen_on_turnloop` legitimately **declines** — a thread that cannot get a
loop of its own keeps the hyper/tokio accept loop. That is the P1 coexistence
rule, and it is precisely why group A's tokio edges still exist. A server that
declined still parks in a turnloop loop, so it still prints `driver=turnloop`,
and every number published from that run would describe a hyper server wearing
the turnloop label.

The runtime already exposed the discriminating quantity and said so in its own
doc comment — `LoopStats::completions`, *"Completions dispatched to a P1 net
subsystem. Zero means turnloop carried no I/O for this process, whatever the turn
count says"* — printed as `completions=N` in the marker line. The harness never
read it. It does now, in two places: at build verification, which already serves
one real request, so a decline fails the run in seconds instead of after hours of
measurement; and per sample, so a run that declined mid-flight cannot count.

Necessary, not sufficient, and the code says so: a non-zero count proves turnloop
carried *some* P1 net I/O in that process, not specifically this listener. It is
still the discriminating quantity available, and strictly better than the marker
alone. Only the turnloop arm is checked — the baseline prints no such line.

The check is self-tested against a synthetic `completions=0` marker, because a
gate nobody has watched fail is not a gate.
