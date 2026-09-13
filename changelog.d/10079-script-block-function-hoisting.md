Initialize block-scoped function declarations at block entry in sloppy scripts
as well as strict code. Forward reads and retained callbacks now use the
correct lexical binding on each loop entry. Annex B's separate outer variable
is still updated at the textual declaration, and block functions correctly
shadow enclosing parameters. Regression coverage pins script/ESM package
contexts and compares native O0, Os, and Oz output with Node.
