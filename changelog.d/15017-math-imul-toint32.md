Math.imul converts its operands with the exact remainder-free ToInt32 (no libm fmod/trunc per call), same results for every double including NaN, infinities and values beyond 2^63.
