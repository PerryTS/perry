Reduced generated module-init code for function metadata (#11927). Function
names and retained `Function.prototype.toString()` sources now live in compact
read-only relative-offset tables, with one runtime batch registration per table
instead of one call and one registry lock acquisition per function. This keeps
dynamic relocation counts unchanged while substantially shrinking large
function-heavy programs.
