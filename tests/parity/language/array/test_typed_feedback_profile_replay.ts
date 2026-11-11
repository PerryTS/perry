function read(xs: any[], i: number): any { return xs[i | 0]; }
const getter: any[] = [0];
Object.defineProperty(getter, "0", { get() { return "getter"; } });
const grown: any[] = [9];
const alias = grown;
for (let i = 0; i < 80; i++) grown.push(i);
const samples: any[] = [[11, 22], ["changed"], [true], [{x: 1}], [], new Array(1), {0: "object"}, new Uint8Array([7]), getter, alias];
const disagree = process.argv.indexOf("disagree") >= 0;
for (let i = 0; i < samples.length; i++) {
    const xs: any = disagree ? samples[i] : samples[0];
    console.log(JSON.stringify(read(xs, 0)));
}
