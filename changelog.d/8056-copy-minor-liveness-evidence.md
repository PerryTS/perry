Fixed moving-GC gate liveness accounting for the current copying-minor
diagnostic format. The gates now count ordinary survivor copies and promotions
as relocation while continuing to reject non-moving whole-block promotions.
