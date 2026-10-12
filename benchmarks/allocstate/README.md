Allocator-state lane at main `3c2f9b065af16f916c7d2cba1462716cacc29d8b`.

Build both arms in separate sources and targets. `remote.py main|head COMMAND`
sets the qb6 arm environment after `bash -lc`. All builds use
`taskset -c 0-23,32-55 cargo build -j8 --release -p perry -p perry-runtime
-p perry-runtime-static -p perry-stdlib-static`. No Mac builds.

Copy `/root/shared/perf-drivers/programs` into HOSTDIR/programs and add
`root-survival.ts` from this directory. `measure.py --hostdir HOSTDIR
--arms main|head compile` checks the 12 drivers, four class micros (using
Node's JS twins), both supplied depth-21 binary-trees forms and root survival.
Only elapsed-time fields are normalized; checksums and other output are exact.
It reuses `scripts/perf_objops_lane.py`'s isolated arm setup. Compilation uses
x86-64-v3, retained symbols, no caches/auto-optimization, and tsc's native root
rewrite ceiling of 2097152 instructions.

Run `measure.py --hostdir HOSTDIR compare` under `taskset -c 0-23,32-55
setarch -R`. It runs five interleaved instruction/RSS samples, disables THP
in the parent before launching both arms, and reports each executable's .text.
`gc` captures full/minor counts with the same program arguments. Failing main
programs are recorded and omitted from the A/B rather than treated as passes.

`count.py --hostdir HOSTDIR --arm main|head` copies the shared plain tree
source at depths 11 and 12 for Callgrind instruction attribution. It verifies
both outputs against Node. This smaller-depth census is distinct from the
full depth-21 performance gate. The number of born nodes at depth D is
`(2**(D+2)-1) + (2**(D+1)-1) + sum(2**(D-d+4)*(2**(d+1)-1)
for d in range(4,D+1,2))`. Two-depth subtraction removes startup costs.
