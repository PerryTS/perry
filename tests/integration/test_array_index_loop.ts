// Perry regression test: arr[i] in for-loop inside function must return correct values
// Bug: when program has many module-level arrays + functions, arr[i] always returns arr[0]
// Requires bloom engine (bloom/core import triggers the larger codegen path)
//
// PASS: prints "PASS" and exits 0
// FAIL: prints "FAIL" and exits 1

import {
  initWindow, closeWindow, beginDrawing, endDrawing,
  clearBackground, setTargetFPS,
  beginMode2D, endMode2D,
} from "bloom/core";
import { Color, Camera2D } from "bloom/core";
import { drawRect, drawCircle, drawTriangle, drawLine, drawRectLines, checkCollisionRecs } from "bloom/shapes";
import { drawText, measureText } from "bloom/text";
import { loadTexture, drawTexturePro, setTextureFilter, FILTER_NEAREST, stageTextures, commitTexture } from "bloom/textures";
import { initAudioDevice, closeAudioDevice, loadSound, playSound, setSoundVolume } from "bloom/audio";
import { clamp, randomFloat, randomInt, lerp } from "bloom/math";
import { Rect, Texture, Sound } from "bloom/core";

// --- Many module-level arrays (mimics a real game) ---
const S0 = [0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0];
const S1 = [0.0,0.0,0.0]; const S2 = [0.0,0.0,0.0,0.0,0.0,0.0,0.0];
const S3: number[] = []; const S4 = [0.0,0.0,0.0,0.0]; const S5 = [0.0,0.0,0.0];
const A0: number[] = []; const A1: number[] = []; const A2: number[] = []; const A3: number[] = [];
const A4: number[] = []; const A5: number[] = []; const A6: number[] = []; const A7: number[] = [];
const B0: number[] = []; const B1: number[] = []; const B2: number[] = []; const B3: number[] = [];
const C0: number[] = []; const C1: number[] = []; const C2: number[] = []; const C3: number[] = [];
const D0: string[] = []; const D1: string[] = [];
const S6 = [0.0, 0.0]; const S7 = [0.0];

// Pre-allocate arrays
for (let i = 0; i < 30; i = i + 1) {
  A0.push(0.0); A1.push(0.0); A2.push(0.0); A3.push(0.0);
  A4.push(0.0); A5.push(0.0); A6.push(0.0); A7.push(0.0);
}
for (let i = 0; i < 100; i = i + 1) {
  B0.push(0.0); B1.push(0.0); B2.push(0.0); B3.push(0.0);
}
for (let i = 0; i < 200; i = i + 1) {
  C0.push(0.0); C1.push(0.0); C2.push(0.0); C3.push(0.0);
}

// --- Many helper functions (to increase code size / function count) ---
function fn0(a: number, b: number): number { if (a > b) return a; return b; }
function fn1(a: number, b: number): number { if (a < b) return a; return b; }
function fn2(a: number): number { if (a < 0.0) return 0.0 - a; return a; }
function fn3(a: number): number { return Math.floor(a); }
function fn4(t: number): number { if (t > 0.5 && t < 4.5) return 1.0; return 0.0; }
function fn5(t: number): number { if (t > 4.5 && t < 6.5) return 1.0; return 0.0; }
function fn6(tx: number, ty: number): number {
  if (tx < 0 || tx >= S4[0] || ty < 0 || ty >= S4[1]) return 0.0;
  const idx = fn3(ty) * fn3(S4[0]) + fn3(tx);
  if (idx < 0 || idx >= S3.length) return 0.0;
  return S3[idx];
}
function fn7(x: number, y: number, vx: number, vy: number, l: number, c: number, s: number): void {
  for (let i = 0; i < 200; i = i + 1) {
    if (C3[i] <= 0.0) { C0[i] = x; C1[i] = y; C2[i] = vx; C3[i] = l; return; }
  }
}
function fn8(x: number, y: number): void {
  for (let i = 0; i < 8; i = i + 1) {
    fn7(x, y, Math.cos(i / 8.0 * 6.28) * 100.0, Math.sin(i / 8.0 * 6.28) * 100.0, 0.3, 1, 3.0);
  }
}
function fn9(x: number, y: number, c: number): void {
  for (let i = 0; i < c; i = i + 1) {
    fn7(x + randomFloat(-8.0, 8.0), y, randomFloat(-60.0, 60.0), randomFloat(-40.0, -10.0), 0.25, 2, 2.0);
  }
}
function fn10(dt: number): void {
  for (let i = 0; i < 200; i = i + 1) {
    if (C3[i] <= 0.0) continue;
    C0[i] = C0[i] + C2[i] * dt; C1[i] = C1[i] + 400.0 * dt; C3[i] = C3[i] - dt;
  }
}
function fn11(): void {
  for (let i = 0; i < 200; i = i + 1) {
    if (C3[i] <= 0.0) continue;
    drawRect(fn3(C0[i]), fn3(C1[i]), 3, 3, { r: 200, g: 200, b: 200, a: 200 });
  }
}
function fn12(): void {
  S3.length = 0;
  for (let i = 0; i < 30; i = i + 1) A5[i] = 0.0;
  for (let i = 0; i < 100; i = i + 1) B2[i] = 0.0;
  for (let i = 0; i < 200; i = i + 1) C3[i] = 0.0;
}
function fn13(s: string, start: number): void {
  let i = start + 0.0; let result = 0.0; let neg = 0.0;
  while (i < s.length) {
    const c = s.charCodeAt(fn3(i));
    if (c > 47.5 && c < 57.5) { result = result * 10.0 + (c - 48.0); i = i + 1.0; } else { break; }
  }
  if (neg > 0.5) result = 0.0 - result;
  S6[0] = result; S6[1] = i;
}
function fn14(dt: number): void {
  for (let i = 0; i < 30; i = i + 1) {
    if (A5[i] < 0.5) continue;
    A0[i] = A0[i] + A2[i] * dt; A7[i] = A7[i] + dt * 4.0;
  }
}
function fn15(t: number): void {
  for (let i = 0; i < 30; i = i + 1) {
    if (A5[i] < 0.5) continue;
    drawRect(fn3(A0[i]), fn3(A1[i]), 32, 32, { r: 200, g: 60, b: 60, a: 255 });
  }
}
function fn16(dt: number): void {
  let la = S0[5] > 0.5 ? 60.0 : -60.0;
  S1[0] = S1[0] + (S0[0] + 16.0 + la - S1[0]) * 6.0 * dt;
  S1[1] = S1[1] + (S0[1] - 20.0 - S1[1]) * 6.0 * dt;
}
function fn17(): void {
  for (let i = 0; i < 20; i = i + 1) {
    const t = i / 19.0;
    drawRect(0, fn3(i * 30.0), 800, 31, { r: fn3(100 + 80 * t), g: fn3(180 + 40 * t), b: 255, a: 255 });
  }
}
function fn18(): void {
  let mi = 0.0;
  while (mi < 12.0) {
    const px = fn3(mi * 180.0 - 180.0);
    drawTriangle(px, 520, px + 90, fn3(520.0 - 80.0), px + 180, 520, { r: 140, g: 160, b: 200, a: 100 });
    mi = mi + 1.0;
  }
}
function fn19(): void {
  for (let i = 0; i < 3; i = i + 1) {
    drawRect(10 + i * 36, 10, 28, 28, { r: 230, g: 50, b: 50, a: 255 });
  }
}

// ============================================================
// THE FUNCTION UNDER TEST — matches drawCollectibles exactly
// ============================================================

function drawItemSpriteFn(frame: number, x: number, y: number, tex: Texture): void {
  drawTexturePro(tex,
    { x: frame * 16, y: 0.0, width: 16, height: 16 },
    { x: x, y: y, width: 32, height: 32 },
    { x: 0.0, y: 0.0 }, 0.0, { r: 255, g: 255, b: 255, a: 255 });
}

function readArrayInLoop(t: number, tex: Texture): number {
  let activeCount = 0.0;
  let flagFound = 0.0;
  let errors = 0.0;

  for (let i = 0; i < 100; i = i + 1) {
    if (B2[i] < 0.5) continue;
    activeCount = activeCount + 1.0;
    const type = B3[i];

    // Exactly match game's drawCollectibles with actual FFI draw calls
    if (type > 9.5 && type < 10.5) {
      const frame = fn3(t * 6.0) % 4;
      drawItemSpriteFn(frame, fn3(B0[i]), fn3(B1[i]), tex);
    } else if (type > 10.5 && type < 11.5) {
      drawItemSpriteFn(4, fn3(B0[i]), fn3(B1[i]), tex);
    } else if (type > 11.5 && type < 12.5) {
      drawItemSpriteFn(5, fn3(B0[i]), fn3(B1[i]), tex);
    } else if (type > 19.5) {
      flagFound = 1.0;
      const fx = fn3(B0[i]);
      const fy = fn3(B1[i]);
      drawRect(fx - 8, fy - 128, 48, 160, { r: 50, g: 255, b: 50, a: 60 });
      drawRect(fx + 14, fy - 120, 5, 152, { r: 160, g: 160, b: 170, a: 255 });
      drawRect(fx + 19, fy - 116, 32, 24, { r: 230, g: 40, b: 40, a: 255 });
      const wave = Math.sin(t * 4.0) * 4.0;
      drawTriangle(fx + 51, fy - 116, fx + 51, fy - 92, fx + 60 + fn3(wave), fy - 104, { r: 210, g: 30, b: 30, a: 255 });
      drawCircle(fx + 16, fy - 124, 6, { r: 255, g: 220, b: 50, a: 255 });
      drawText("GOAL", fx - 2, fy - 148, 18, { r: 255, g: 255, b: 50, a: 255 });
      if (fx < 1823.5 || fx > 1824.5) {
        errors = errors + 1.0;
      }
    }

    // Validate: inactive slots should not be visited
    if (i > 15 && i < 100) {
      errors = errors + 1.0;
    }
  }

  if (activeCount < 4.5 || activeCount > 5.5) errors = errors + 1.0;
  if (flagFound < 0.5) errors = errors + 1.0;
  return errors;
}

// ============================================================
// SETUP + RUN
// ============================================================

// Set test data (B0=CX, B1=CY, B2=CA, B3=CT)
B2[0] = 1.0; B3[0] = 10.0; B0[0] = 256.0; B1[0] = 384.0;
B2[1] = 1.0; B3[1] = 10.0; B0[1] = 288.0; B1[1] = 384.0;
B2[2] = 1.0; B3[2] = 10.0; B0[2] = 320.0; B1[2] = 384.0;
B2[3] = 1.0; B3[3] = 11.0; B0[3] = 960.0; B1[3] = 224.0;
B2[15] = 1.0; B3[15] = 20.0; B0[15] = 1824.0; B1[15] = 384.0;

// Setup tiles
S4[0] = 60.0; S4[1] = 15.0;
while (S3.length < 900) S3.push(0.0);
for (let x = 0; x < 60; x = x + 1) { S3[13 * 60 + x] = 1.0; S3[14 * 60 + x] = 2.0; }

// Enemy
A5[0] = 1.0; A4[0] = 1.0; A0[0] = 480.0; A1[0] = 384.0; A2[0] = 60.0;

initWindow(400, 300, "test");
setTargetFPS(60);
initAudioDevice();

const texItems = loadTexture("assets/sprites/items.png");
setTextureFilter(texItems, FILTER_NEAREST);

const camera: Camera2D = {
  offset: { x: 200.0, y: 150.0 },
  target: { x: 400.0, y: 300.0 },
  rotation: 0.0,
  zoom: 1.0,
};

// Run the test inside the game loop (like the real game does)
const RESULT = [0.0, 0.0]; // [tested, errors]
let frameNum = 0.0;

while (!windowShouldClose()) {
  const dt = 0.016;
  const t = frameNum * 0.016;
  frameNum = frameNum + 1.0;

  // State machine like the game
  const state = 2.0; // ST_PLAYING

  beginDrawing();
  clearBackground({ r: 40, g: 40, b: 60, a: 255 });

  if (state > 1.5 && state < 2.5) {
    fn16(dt);
    fn14(dt);
    fn10(dt);

    camera.target.x = fn3(S1[0]);
    camera.target.y = fn3(S1[1]);

    fn17();
    fn18();

    beginMode2D(camera);

    // Draw tiles
    for (let ty = 0; ty < 15; ty = ty + 1) {
      for (let tx = 0; tx < 60; tx = tx + 1) {
        const tile = fn6(tx, ty);
        if (tile > 0.5) drawRect(tx * 32, ty * 32, 32, 32, { r: 100, g: 200, b: 60, a: 255 });
      }
    }

    // THE FUNCTION UNDER TEST — called from within beginMode2D + state machine
    if (RESULT[0] < 0.5) {
      RESULT[1] = readArrayInLoop(t, texItems);
      RESULT[0] = 1.0;
    }

    // Also call the draw functions like the game does
    fn15(t);
    fn11();

    endMode2D();
    fn19();
  }

  endDrawing();

  // Exit after first frame that ran the test
  if (RESULT[0] > 0.5) break;
}

closeAudioDevice();
closeWindow();

if (RESULT[1] < 0.5) {
  console.log("PASS");
} else {
  console.log("FAIL: errors=" + RESULT[1].toString());
}
