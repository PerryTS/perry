const out: any = process.stdout;
if (typeof out.hasColors === "function") {
  console.log("hasColors object:", out.hasColors({ colors: 16 }));
  console.log("depth object:", out.getColorDepth({ TMUX: "1" }));
} else {
  console.log("hasColors type:", typeof out.hasColors);
}
