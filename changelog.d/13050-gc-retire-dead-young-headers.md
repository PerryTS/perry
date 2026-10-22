A full sweep now retires a dead object's header in an eden, survivor or
long-lived block (`obj_type 0`, size kept), as it already did for old holes.
Before this change, every later full before the block reset swept the object
again: it re-ran its payload finalizer and counted its bytes as freed again.
On the compiled opencode TUI the idle reducer therefore scored every idle full
productive (~10.5 MB "freed" with ~0.2 MB allocated) and ran a whole-heap cycle
every 15 s indefinitely. TUI main-thread instructions over 60 s of idle drop
from 2574M to 647M. Test:
`gc::tests::incremental_sweep_reclaim::full_sweep_frees_a_dead_young_object_once`.
