Strict equality against a proven Number now compares raw double encodings directly and
matches compact INT32 values against the proven operand's exact integer encoding.
This removes speculative decoding of the varying operand, shortening the byte
comparison dependency that slowed scanner loops despite fewer instructions.
Saturating conversion plus an exact round trip preserves NaN, infinities,
fractions, signed zero and integer bounds. Checked byte admission, B4 owner
liveness and statepoint relocation retain their existing contracts.
