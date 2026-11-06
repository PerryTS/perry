// Test: Module-level array reads with loop index inside a function
// Variation: untyped arrays (no `: number[]` annotation) + many module vars

// Module-level arrays WITHOUT type annotations
const CX = [];
const CY = [];
const CT = [];
const CA = [];

// Additional module-level state (to increase module var count)
let playerX = 100.0;
let playerY = 200.0;
let cameraX = 0.0;
let cameraY = 0.0;
let score = 0;
let lives = 3;
let level = 1;
let gameOver = false;
const tileW = 32;
const tileH = 32;
const mapW = 64;
const mapH = 20;

// Pre-allocate at module level via push loop
for (let i = 0; i < 100; i = i + 1) {
  CX.push(0.0);
  CY.push(0.0);
  CT.push(0.0);
  CA.push(0.0);
}

// Set some values (simulating level loading)
CX[15] = 1824.0;
CY[15] = 384.0;
CT[15] = 20.0;
CA[15] = 1.0;

CX[50] = 999.0;
CY[50] = 888.0;
CT[50] = 25.0;
CA[50] = 1.0;

let totalDrawn = 0;

function drawRect(x: number, y: number, w: number, h: number): void {
  totalDrawn = totalDrawn + 1;
}

// Function that reads arrays with loop index
function drawCollectibles(): void {
  let i = 0;
  while (i < 100) {
    if (CA[i] > 0.5) {
      const type_val = CT[i];
      if (type_val > 19.5) {
        drawRect(CX[i], CY[i], 32, 32);
      }
    }
    i = i + 1;
  }
}

// For-loop variant with continue
function updateCollectibles(px: number, py: number): number {
  let collected = 0;
  for (let i = 0; i < 100; i = i + 1) {
    if (CA[i] < 0.5) continue;
    const dx = CX[i] - px;
    const dy = CY[i] - py;
    const dist = dx * dx + dy * dy;
    if (dist < 1024.0) {
      CA[i] = 0.0;
      collected = collected + 1;
    }
  }
  return collected;
}

// Verify direct reads
console.log(CX[15]); // 1824
console.log(CY[15]); // 384

// Game loop — call function each frame
let frame = 0;
while (frame < 3) {
  drawCollectibles();
  frame = frame + 1;
}
console.log(totalDrawn); // 6

// For-loop with continue
const collected = updateCollectibles(1824.0, 384.0);
console.log(collected); // 1
console.log(CA[15]); // 0

// After collection
totalDrawn = 0;
drawCollectibles();
console.log(totalDrawn); // 1
