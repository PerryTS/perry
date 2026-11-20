Collapse the generic (untyped) property-get inline cache from a 33-block diamond
with six runtime call sites to the inline hit path plus the polymorphic ways and
two out-of-line exits: `js_object_get_field_ic_nonptr` for non-pointer receivers
(SSO, class refs, nullish throw, primitives) and `js_object_get_field_ic_slow`
for a pointer receiver that failed a guard (overflow slot, Array-subclass
named-prefix proofs, miss and prime). Per site: 191 → 106 IR instructions,
6 → 2 call sites. The packed-MRU hit path, the ways and every cache decision are
unchanged; the field address is emitted as a typed `gep` so instruction selection
keeps the scaled addressing mode. On @babel/parser `.text` shrinks 14.4 % and
`.perry_gcmap` 11.7 %; total benchmark instructions drop 0.5 % (`interp.ts`
−1.5 %) with RSS unchanged. A new gap test exercises every arm that moved out of
line against Node.
