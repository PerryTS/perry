### Added — the warm-process soak arm #8163's residual needs

`tests/release/packages/next-app-route/fixture.sh` gains `run_warm_soak`: one warm
process, N verifier passes, default GC. Off by default
(`PERRY_NEXT_ROUTE_WARM_PASSES=0`); it is the acceptance instrument for closing
#8163, not a per-run check.

Two measurements motivate it. **In normal mode the cold-start loop's two verifier
passes run zero copying minors**, so that arm cannot exercise a moving-GC holder
at all — only the forced arm moves anything. And #8163's residual costs roughly
one broken request per hundred verifier passes, so even a hundred passes is not a
verdict: at p = 0.01 a clean 100-pass run happens ~37% of the time on a
known-broken build, and two of five measured runs did come back clean while the
bug was present.

The arm therefore prints the confidence its N actually buys (rule of three: ~300
passes for 95%, ~460 for 99%) rather than letting a green run imply elimination,
asserts its subject was live (`copying minors > 0`, the rule the forced arm's
evacuation-liveness assert already applies), and reports an observed failure ahead
of that liveness complaint so a real broken request is never masked by "exercised
nothing".

Validated against real provider images in both directions: it catches a pre-fix
build (pass 19), catches the surviving #8163 residual on the merged fix (pass 3 at
N=100, pass 6 at N=10 — an independent local reproduction of the bench-mini
measurement), stays quiet at N=0, and both asserts fire when sabotaged.
