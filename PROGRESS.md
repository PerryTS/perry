# BRG progress

Source7d66c8a0c8ef60c9ebb6e8e7b8f36f0998768e4b, based on current-main4268659de via merge92ffaaaae.
DB probe and Symbol mechanisms fixed generically; no push.
Runtime6190/Perry1324 active pass; CG2589/WPF561 pass with unchanged compiler.
JSC22/22 both arms; Windows/ABI pass; facts52 exact, full26tdiff outputs equal,
no new per-unit-stronger site. Startup five deterministic n3 witnesses <= main.

Native six-pair sweep complete, all180 samples retained. Thirteen AWFY cases pass
the paired-cycle limit. NBody initial clean median+3.904%, positive clean range.
Its frontend/code-placement effect is now explained: clean Icache51.150M->105.252M,
frontend12.5%->17.7%, hot-order controls reduce clean cost to+0.293/+0.440%.
All eight sampled functions' instructions/constants stay equal within each arm's
controls after validated relocations. No order/alignment tuning ships.
A candidate bundle/report is being prepared for the remaining acceptance decision.

DB n5 instructions27.664B->26.775B (<pre27.033B). Property788.642M is+0.2673%
versus pre, within the explicit003 acceptance; flat versus integrated main.
Only existing metadata owners are used; inferred-name copy success and UTF8 prove
canonical equality, with actual code-key and image-name guards.

All commits Ralph Küpper <ralph3@skelpo.com>, no attribution. Full failures/partial/
contended evidence retained. Inactive dumps gzip+SHA verified; floor25GB monitored.
Ball remains separate design-before-build under002/003. No Ball candidate built.
