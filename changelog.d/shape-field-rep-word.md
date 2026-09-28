Shape records carry a per-slot field-representation word (two bits per inline
slot 0..32), and it is part of shape identity. Nothing produces a non-`Any`
representation yet, and an all-`Any` shape keeps exactly the identity key it
had before, so no program's shapes change. The record grows from 40 to 48
bytes.
