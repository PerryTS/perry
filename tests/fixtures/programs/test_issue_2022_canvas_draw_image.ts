// Issue #2022 — Canvas.drawImage + loadImage API shape.
// Locks Promise<Image>, image metadata, and the three HTML Canvas-compatible drawImage overloads.
import { App, Canvas, loadImage, type Image } from "perry/ui";

const spritePromise = loadImage("assets/sprite.png");
spritePromise.then((img) => {
  console.log(img.ready, img.width, img.height);
});
const sprite: Image = await spritePromise;

const canvas = Canvas(128, 128);
if (sprite.ready) {
  canvas.drawImage(sprite, 0, 0);
  canvas.drawImage(sprite, 8, 8, sprite.width, sprite.height);
  canvas.drawImage(sprite, 0, 0, 16, 16, 48, 48, 32, 32);
}

App({ title: "Canvas drawImage", root: canvas }).run();
