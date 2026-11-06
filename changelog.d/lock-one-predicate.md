The built-in prototype lock (DEC 250) is one decision, `perry_abi::lock::refuses`: the runtime asks it before a program
mutation of a sealed intrinsic prototype, and the whole-program facts ask it (`final_on`) whether a read on an intrinsic
chain is final. Behaviour and facts are unchanged; the two hand-written copies of the table are gone.
