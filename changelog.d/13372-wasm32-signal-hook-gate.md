`perry-runtime` builds for `wasm32` again: the signal-completion hook slot added
by #13312 names `turnloop_proc`, which does not exist on wasm32, so it is now
gated the same way as its installer and its only caller.
