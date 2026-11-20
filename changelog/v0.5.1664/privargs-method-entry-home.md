Remove home-context entry and exit from declaration class method wrappers.
Their receiver is already explicit, and private brands and slots are shape
facts. Fresh class evaluations retain their lexical home scope for extracted
and borrowed methods. This removes the redundant context lookup and stack
push/pop on bound declaration methods without adding runtime state.

Validation covers extracted and bound methods, wrong receivers, shape changes,
fresh evaluation isolation, nested callbacks, exceptions and derived arrows.
