The masked-window region's flow-refined Number locals (#6750) are no longer a
separate set: a fast copy admits a refined local to its own Number-local scope
and withdraws it at the first write it cannot prove Number, so
`type_analysis::local_is_number` is the one query for every Number local and
`FnCtx::masked_region_scalar_locals` is deleted (charter step 5L, P5).
