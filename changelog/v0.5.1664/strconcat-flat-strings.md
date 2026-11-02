Remove redundant primitive conversion and materialization from dynamic string + string by using the shared SSO/flat-string concat path. Canonical WTF-8 inputs need surrogate repair only when a lead/trail pair forms at the join; raw unmerged pairs retain the full repair path.

String length reads reuse header metadata, and number/boolean length reads return undefined directly when live prototype shapes prove the property absent. Prototype additions and getters retain ordinary property semantics. Inline ASCII charCodeAt requires the producer's encoding proof and returns NaN directly for numeric out-of-bounds indices; Unicode and coercing indices retain the decoder.

Coverage includes split and lone surrogates, raw interior pairs, primitive prototype mutations, argument coercion, erased types, and million-character flat strings. No string representation, header layout, cache, or version changes.

Array.join now preserves the existing encoding proof when all copied pieces carry it, including the surrogate-repair builder. This keeps joined ASCII strings on the inline charCodeAt path without certifying unknown raw inputs.
