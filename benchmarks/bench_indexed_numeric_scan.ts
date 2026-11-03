// Synthetic numeric-array access reduction; contains no application source.
// Each variant visits the same cells in the same order and returns early on 2.
const STEPS: number[] = [-17, -16, -15, -1, 1, 15, 16, 17];
const TYPED_STEPS = new Int32Array([-17, -16, -15, -1, 1, 15, 16, 17]);
class AccessBoard {
  cells: number[] = new Array<number>(128).fill(0);
  typed: Int32Array = new Int32Array(128);
  constructor() {
    for (let i = 0; i < 128; i++) {
      const value = (i & 0x88) === 0 ? ((i * 3) % 7) - 3 : 0;
      this.cells[i] = value;
      this.typed[i] = value;
    }
  }
  at(square: number): number { return this.cells[square] ?? 0; }
  method(origin: number): boolean {
    for (let j = 0; j < STEPS.length; j++) {
      const square = origin + (STEPS[j] ?? 0);
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && this.at(square) === 2) return true;
    }
    return false;
  }
  field(origin: number): boolean {
    for (let j = 0; j < STEPS.length; j++) {
      const square = origin + (STEPS[j] ?? 0);
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && (this.cells[square] ?? 0) === 2) return true;
    }
    return false;
  }
  local(origin: number): boolean {
    const cells = this.cells;
    for (let j = 0; j < STEPS.length; j++) {
      const square = origin + (STEPS[j] ?? 0);
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && (cells[square] ?? 0) === 2) return true;
    }
    return false;
  }
  liveBound(origin: number): boolean {
    const cells = this.cells;
    for (let j = 0; j < STEPS.length; j += 1) {
      const square = origin + (STEPS[j] ?? 0);
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && (cells[square] ?? 0) === 2) return true;
    }
    return false;
  }
  typedLocal(origin: number): boolean {
    const cells = this.typed;
    for (let j = 0; j < STEPS.length; j++) {
      const square = origin + (STEPS[j] ?? 0);
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && (cells[square] ?? 0) === 2) return true;
    }
    return false;
  }
  directions(origin: number): boolean {
    const cells = this.cells;
    for (let j = 0; j < 8; j += 1) {
      const delta = j < 3 ? j - 17 : j < 5 ? 2 * j - 7 : j + 10;
      const square = origin + delta;
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && (cells[square] ?? 0) === 2) return true;
    }
    return false;
  }
  fixedBound(origin: number): boolean {
    const cells = this.cells;
    for (let j = 0; j < 8; j += 1) {
      const square = origin + (STEPS[j] ?? 0);
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && (cells[square] ?? 0) === 2) return true;
    }
    return false;
  }
  fixedTyped(origin: number): boolean {
    const cells = this.cells;
    for (let j = 0; j < 8; j += 1) {
      const square = origin + (TYPED_STEPS[j] ?? 0);
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && (cells[square] ?? 0) === 2) return true;
    }
    return false;
  }
  arithmetic(origin: number): boolean {
    // Same-answer control only: computes this fixture's immutable cell value.
    // It cannot replace reading the mutable board in the real chess engine.
    for (let j = 0; j < 8; j += 1) {
      const delta = j < 3 ? j - 17 : j < 5 ? 2 * j - 7 : j + 10;
      const square = origin + delta;
      if (square >= 0 && square < 128 && (square & 0x88) === 0 && ((square * 3) % 7) - 3 === 2) return true;
    }
    return false;
  }
}
function flat(cells: number[], origin: number): boolean {
  for (let j = 0; j < STEPS.length; j++) {
    const square = origin + (STEPS[j] ?? 0);
    if (square >= 0 && square < 128 && (square & 0x88) === 0 && (cells[square] ?? 0) === 2) return true;
  }
  return false;
}
function scan(board: AccessBoard, amount: number, variant: string): number {
  let sum = 0;
  // Select the variant outside the timed hot loop: no per-probe mode branch.
  if (variant === "method") {
    for (let i = 0; i < amount; i++) if (board.method(i & 127)) sum++;
  } else if (variant === "field") {
    for (let i = 0; i < amount; i++) if (board.field(i & 127)) sum++;
  } else if (variant === "local") {
    for (let i = 0; i < amount; i++) if (board.local(i & 127)) sum++;
  } else if (variant === "live-bound") {
    for (let i = 0; i < amount; i++) if (board.liveBound(i & 127)) sum++;
  } else if (variant === "typed-local") {
    for (let i = 0; i < amount; i++) if (board.typedLocal(i & 127)) sum++;
  } else if (variant === "flat") {
    const cells = board.cells;
    for (let i = 0; i < amount; i++) if (flat(cells, i & 127)) sum++;
  } else if (variant === "directions") {
    for (let i = 0; i < amount; i++) if (board.directions(i & 127)) sum++;
  } else if (variant === "arithmetic") {
    for (let i = 0; i < amount; i++) if (board.arithmetic(i & 127)) sum++;
  } else if (variant === "fixed-bound") {
    for (let i = 0; i < amount; i++) if (board.fixedBound(i & 127)) sum++;
  } else if (variant === "fixed-typed") {
    for (let i = 0; i < amount; i++) if (board.fixedTyped(i & 127)) sum++;
  } else throw new Error("Unknown access variant");
  return sum;
}
const variant = process.argv[2] ?? "method";
const amount = Number(process.argv[3] ?? "5000000");
if (!Number.isSafeInteger(amount) || amount <= 0) throw new Error("Expected a positive iteration count");
const board = new AccessBoard();
const warmup = scan(board, 100000, variant);
const started = performance.now();
const checksum = scan(board, amount, variant);
const milliseconds = performance.now() - started;
console.log(JSON.stringify({ variant, amount, warmup, checksum, milliseconds }));
