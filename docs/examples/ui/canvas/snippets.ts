// demonstrates: per-API Canvas drawing snippets shown in docs/src/ui/canvas.md
// docs: docs/src/ui/canvas.md
// platforms: macos, linux
// run: false

// `run: false` because Canvas drawing only paints visible pixels when the
// widget is attached to a run loop and the host window is visible. Compile-link
// is enough to certify the codegen surface; this file pins every Canvas
// instance method down so a future rename / drop trips a link error in CI.

// ANCHOR: imports
import { App, Canvas } from "perry/ui"
// ANCHOR_END: imports

// ANCHOR: create
const canvas = Canvas(400, 300)
canvas.setFillColor(1.0, 0.4, 0.0, 1.0)
canvas.fillRect(10, 10, 100, 80)
// ANCHOR_END: create

// ANCHOR: rectangles
canvas.setFillColor(1.0, 0.0, 0.0, 1.0)    // red
canvas.fillRect(10, 10, 100, 80)

canvas.setStrokeColor(0.0, 0.0, 1.0, 1.0)  // blue
canvas.setLineWidth(2)
canvas.strokeRect(150, 10, 100, 80)
// ANCHOR_END: rectangles

// ANCHOR: lines
canvas.setStrokeColor(0.0, 0.0, 0.0, 1.0)
canvas.setLineWidth(1)
canvas.beginPath()
canvas.moveTo(10, 10)
canvas.lineTo(200, 150)
canvas.stroke()
// ANCHOR_END: lines

// ANCHOR: arcs
canvas.setFillColor(0.0, 1.0, 0.0, 1.0)
canvas.beginPath()
canvas.arc(200, 150, 50, 0, Math.PI * 2)  // x, y, radius, startAngle, endAngle
canvas.fill()
// ANCHOR_END: arcs

// ANCHOR: gradient
canvas.beginPath()
canvas.moveTo(0, 200)
canvas.lineTo(100, 120)
canvas.lineTo(200, 160)
canvas.lineTo(400, 60)
// r1, g1, b1, a1, r2, g2, b2, a2, direction (0 = vertical, 1 = horizontal)
canvas.fillGradient(0.2, 0.4, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0)
// ANCHOR_END: gradient

// ANCHOR: text
canvas.setFillColor(0.0, 0.0, 0.0, 1.0)
canvas.setFont("16px sans-serif")
canvas.fillText("Hello Canvas!", 50, 50)
// ANCHOR_END: text

App({
    title: "Canvas Demo",
    width: 400,
    height: 300,
    body: canvas,
})
