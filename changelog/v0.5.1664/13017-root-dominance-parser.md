fix(gc-check): recognize managed selects and stack-home addresses

Recognize pointer-valued selects and tail-marked calls in the root-dominance
checker. Accept direct stack-home alloca addresses in gc-live while continuing
to reject unknown operands and non-alloca AS0 pointers.
