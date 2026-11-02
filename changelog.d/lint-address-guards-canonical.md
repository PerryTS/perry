Route the util.parseArgs and util.styleText heap-type probes and the born-null
prototype test through the canonical address classifier, so a native handle in
the handle band is rejected before any GC header read. The instanceof
Function.prototype probe now tests for a reference rather than a raw heap tag.
