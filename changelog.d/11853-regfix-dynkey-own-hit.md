Computed string-key reads (`o[k]`) of an own key the receiver's shape could not
match by word (a non-atom key, or one past the inline positions) no longer look
the key up twice. The receiver's own lookup, which already ran to prove a key
absent, now answers a present key from its slot and files it in the read stub,
so the next read of that key is one stub probe. The `lit_get_set_has` row of
#10506 goes from 4,188 to 2,089 instructions per operation (3,050 before the
computed-key entry existed).
