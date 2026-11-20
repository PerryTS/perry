On Apple aarch64, codegen again reuses a proof defined in another block of
the body being lowered (a receiver run's, a constructor run's carried check,
the bind watch). A body there opens with an activation start whose branch
into the body is emitted only after the body is lowered; the dominance query
now counts that pending edge instead of treating the whole body as
unreachable and refusing every such proof.
