**gc: pretenure-accumulator admission infrastructure (#7598, scope-reduced per audit)**

Adds the static admission machinery for a future pretenurer, with no
allocator or codegen consumers: `collect_pretenure_accumulator_locals`
(accumulator `let` at loop depth 0, every push at depth ≥ 1, layered on the
all-pointer terms, refusal tests for the per-iteration and mixed-depth
shapes) and an explicit `region_runs_once` parameter on both fact-graph
builders — module main/init pass true, every function/method/closure region
false, with a graph-level test pinning both polarities. Only a run-once
region makes "declared outside every loop" a cohort-lifetime claim; a
function region's accumulator is re-entered per call (measured 6.6× slower
when pretenured).

The originally proposed born-tenured allocation was removed after audit:
json_pipeline's minor-moved cohort is the runtime-allocated parse tree
(~113 MB), not codegen-visible literals (~1 MB live at minor time), and the
PR's measured win was a confound between arms that differed in whether
#7613's promote-on-first-copy seed fired. The deferred-page-registration
finding is extracted separately. Next routes for #7598: dynamic feedback or
allocation-context pretenure inside the JSON materialiser.
