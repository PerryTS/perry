// Run with an instrumented runtime, every-poll seeded collection, from-space
// protection, and a 2 GiB process address-space ceiling. A missing idle-Eden
// reclaim exhausts that ceiling; a healthy run must finish and match node.
class ProtectedCell {
    value: number;
    constructor(value: number) { this.value = value; }
}
const PROTECTED_CELLS: ProtectedCell[] = [
    new ProtectedCell(3), new ProtectedCell(5),
    new ProtectedCell(7), new ProtectedCell(11),
];
function protectedPolls(n: number): number {
    let sum = 0;
    const keep: any[] = [];
    for (let i = 0; i < n; i++) {
        const cell: ProtectedCell = PROTECTED_CELLS[i & 3];
        sum += cell.value;
        keep.push({ index: i, text: "poll" + i });
        if (keep.length > 8) keep.length = 0;
    }
    return sum + keep.length;
}
console.log(protectedPolls(8192));
