// Runtime setters own their precise store barrier, including closure writeback.
function run() {
    let value: any = { text: "first" }
    let items: any[] = []
    let text = ""
    function write(i: number) {
        value = { text: "value" + i, inner: { n: i } }
        items.push(value)
        items.unshift(value)
        text += "x"
        return value.inner.n
    }
    let sum = 0
    for (let i = 0; i < 80; i++) {
        if (i % 8 === 0) (globalThis as any).gc?.()
        sum += write(i)
    }
    (globalThis as any).gc?.()
    console.log(sum, items.length, value.inner.n, text.length)
}
run()
