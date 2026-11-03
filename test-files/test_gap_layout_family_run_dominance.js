class Input {
  constructor(css) {
    this.css = css.toString();
    if (this.css === "bom") {
      this.hasBOM = true;
      this.css = this.css.toString();
    } else {
      this.hasBOM = false;
    }
    this.document = this.css;
  }
}
for (const css of ["bom", "plain"]) {
  const input = new Input(css);
  console.log(input.css, input.hasBOM, input.document);
}
