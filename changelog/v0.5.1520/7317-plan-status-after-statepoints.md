Records where `docs/engine-plan.md` actually stands after layers 0 and 2 landed
(#7301 in-process LLVM, #7305 invoke/landingpad EH, #7314 statepoints opt-in).

States what #7314 establishes — every root path fails closed, metadata re-encoded
4,214,384 -> 224,832 B (18.7x), 23,301/23,301 safepoints as statepoints with zero
fallbacks — and, as plainly, what it does not: binary size is a wash rather than a
win, and statepoints describe emitted frames only, so hand-written runtime Rust
(layer 3) is untouched and is where #7280's fault has already moved.

Names the three things blocking adoption, none of which are code: four of five new
knobs have no CI arm (kill-policy), `gc-native-roots` is not a required context,
and #7314 pushed two codegen files over the file-size cap.
