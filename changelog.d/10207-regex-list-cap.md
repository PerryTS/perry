Removed the fixed entry cap on the regex runtime's operation lists, which made
`String.prototype.replace`, `replaceAll` and `split` throw "Regular expression
memory limit exceeded" once their output reached 8,388,608 list entries — a
callback `replace` over about 700,000 short records. Those lists are ordinary GC
arrays and are now bounded by allocation and the maximum string length, like
the rest of the runtime.
