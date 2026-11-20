**Receiver hoists now use the shared safepoint-region model** (#9254 phase 2).
The packed/versioned loop clone's rooted receiver box, pre-masked base handle
and poll reload recipe now live in one active descriptor entry instead of three
parallel `FnCtx` maps. Fired back-edge polls ask the shared boundary algebra to
admit every cached address before refreshing it, and nested clones reuse outer
descriptors without shortening their lifetime. Generated behavior is unchanged;
this is the first lowering consumer of the phase-1 model.
