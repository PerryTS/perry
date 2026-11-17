# Mixed I32 field layouts

The unknown-receiver property probe derives direct-read and direct-write entries
per slot. A shape containing an I32 field can still answer its ordinary number
and function fields directly. I32 slots use the existing layout accessor, which
returns a canonical JavaScript Number. Their entries never carry a raw-load or
raw-store flag, and a nonempty layout with I32 slots always uses the indexed form so the
unindexed positional bound cannot expose its encoded words.

An inherited read follows the same slot rule. A raw-location memo refuses an I32
slot because its representation requires the layout accessor. This decision is
made while deriving the entry; ordinary direct hits acquire no representation
test. Stores of an encoded field retain the checked encoding path.
