// #10152: the export getter claims `read`'s symbol, so function deduplication
// discards its body after lowering has already interned a dispatch descriptor.
function read(value: any) {
    return value.segment();
}
const readAlias = read;
export { readAlias as read };

// Keep unit 0 larger than the string initializer so `segment`'s bytes go to
// unit 1 when the compile regression forces PERRY_CODEGEN_UNITS=2.
export function pad(value: any) {
    return value.alpha + value.beta + value.gamma;
}
