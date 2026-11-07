The facts dump now names the source call for calls the analysis makes on a
program call's behalf, such as the callback a built-in `replace` runs when it
is reached through an extracted method and `.call`. Those calls used to show
no source call, so the ledger could not place them on a source line. Compiled
programs and the facts are unchanged.
