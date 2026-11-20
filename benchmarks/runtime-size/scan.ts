// Standalone synthetic reduction: short numeric-array scans behind class methods.
const STEPS: number[] = [-17, -16, -15, -1, 1, 15, 16, 17];
class ProbeBoard {
  private cells: number[] = new Array<number>(128).fill(0);
  constructor() {
    for (let i = 0; i < 128; i++) this.cells[i] = (i & 0x88) === 0 ? ((i * 3) % 7) - 3 : 0;
  }
  at(square: number): number { return this.cells[square] ?? 0; }
  probeForOf(origin: number): boolean {
    for (const delta of STEPS) {
      const square = origin + delta;
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && this.at(square) === 2) return true;
    }
    return false;
  }
  probeIndexed(origin: number): boolean {
    for (let j = 0; j < STEPS.length; j++) {
      const square = origin + (STEPS[j] ?? 0);
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && this.at(square) === 2) return true;
    }
    return false;
  }
}
function scan(board: ProbeBoard, iterations: number, indexed: boolean): number {
  let checksum = 0;
  if (indexed) {
    for (let i = 0; i < iterations; i++) if (board.probeIndexed(i & 127)) checksum++;
  } else {
    for (let i = 0; i < iterations; i++) if (board.probeForOf(i & 127)) checksum++;
  }
  return checksum;
}
const mode = process.argv[2] ?? "forof";
const iterations = Number(process.argv[3] ?? "5000000");
if (mode !== "forof" && mode !== "indexed") throw new Error("Expected forof or indexed");
if (!Number.isSafeInteger(iterations) || iterations <= 0) throw new Error("Expected positive iteration count");
const board = new ProbeBoard();
const warmup = scan(board, 100000, mode === "indexed");
const started = performance.now();
const checksum = scan(board, iterations, mode === "indexed");
const milliseconds = performance.now() - started;
console.log(JSON.stringify({ mode, iterations, warmup, checksum, milliseconds }));
