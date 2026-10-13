Predicted class-field reads in a straight-line memo run share the receiver's
managed handle along with the shape proof. Later hits address slots from that
handle instead of repeating the NaN-box receiver subtraction. The handle stays
in LLVM's managed pointer domain throughout the run; mem2reg and statepoints
compute its liveness and relocate it at collecting calls. Calls still revoke
the shape proof, and the complete site rederives the receiver before renewing
it. A gap test puts an evacuating getter between two reads of one receiver.
The native base remains a tagged JS value; the unboxed handle is its managed
derivation, biased for addressing. The existing base/derived rewriter accepts
bounded offsets in both directions, retaining its 64 MiB bound.
A runtime test covers that mixed-encoding relocation and retains the offset
bound. The IR test includes a negative control that removes the managed types
and demonstrates that the same collecting call then has no relocation.
Large NaN-box pointer offsets use the same existing immutable operand table as
integer operands; no per-site immediate or extra constant register is emitted.
The handle conversion is pure only within this continuous managed lifetime,
and sharing happens after statepoints establish relocation.
