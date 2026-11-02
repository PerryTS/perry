On macOS, modules that declare a named function compile again. The compact
stack-map rewriter refused the `.alt_entry` line that Mach-O prints for the
function-name aliases. It now accepts every symbol attribute LLVM can emit.
On every target except x86_64 Linux, a throw out of a `for...of` body now
reaches the catch with its error value instead of `undefined`.
