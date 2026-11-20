### Faster

- Building the output of a `String.prototype.replace` spends about 28% fewer instructions with a string replacement, and 5-7% fewer with a callback. Both passes over the output's pieces asked the collector whether it was due to run once per piece, and a piece is usually a few characters, while answering that question costs about 650 instructions. It is now asked once per 512 characters read, which is the bound it was always meant to keep. Peak memory is unchanged (#10165).
