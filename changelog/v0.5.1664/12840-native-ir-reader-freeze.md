A function body can reach the native reader as rendered text lines, not
only as typed instructions. The line reader had no `freeze` arm, so a
`blk.freeze` line (typed-array element indices) fell through to the
binary-op parser and the whole module failed with "bad binary op `freeze`
operands". Claude Code 2.1.112 hit it in `indexPath$generic` and did not
compile. Both paths now share one LLVMBuildFreeze helper; a reader
round-trip test covers the text form.
