const x = process.env.SNAP22_ABORT_SENTINEL;
const y = process.argv;
process.abort();
export { x, y };
