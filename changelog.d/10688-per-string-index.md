The UTF-16 index no longer lives in a fixed four-slot cache that evicted
round-robin. Interleaving indexed access across more strings than it held
rebuilt the index from scratch on every access — 1,224x slower from the fifth
string onward, as a step function. Entries now live until their string dies and
are reclaimed by the collector's existing prune hook, so the cliff cannot occur
at any number of strings (#10688).
