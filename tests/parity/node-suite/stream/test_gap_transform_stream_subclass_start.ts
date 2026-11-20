// `class X extends TransformStream { constructor() { super({ start, transform }) } }`:
// the transformer's start(controller) runs at construction (eventsource-parser's
// EventSourceParserStream builds its parser there), and `super(...)` to a WHATWG
// stream base constructs instead of throwing "Super constructor is not a constructor".
class LineCounter extends TransformStream<string, string> {
  constructor() {
    let seen: { n: number } | undefined
    super({
      start() { seen = { n: 0 } },
      transform(chunk, controller) {
        seen!.n++
        controller.enqueue(`${seen!.n}:${chunk}`)
      },
    })
  }
}
class Upper extends TransformStream<string, string> {
  constructor() { super({ transform(chunk, controller) { controller.enqueue(chunk.toUpperCase()) } }) }
}
class Plain extends TransformStream {}

async function collect(stream: ReadableStream<string>) {
  const out: string[] = []
  const reader = stream.getReader()
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    out.push(value)
  }
  return out
}

async function main() {
  const body = new Response("data: a\n\ndata: b\n\n").body!
  const lines = await collect(body.pipeThrough(new TextDecoderStream()).pipeThrough(new Upper()).pipeThrough(new LineCounter()))
  console.log("lines", JSON.stringify(lines))
  try { new Plain(); console.log("plain ok") } catch (e) { console.log("plain threw", (e as Error).message) }
  function nested() {
    class Inner extends TransformStream<string, string> {
      constructor() { super({ transform(c, ctl) { ctl.enqueue(c + "!") } }) }
    }
    return new Inner()
  }
  const out = await collect(new Response("x").body!.pipeThrough(new TextDecoderStream()).pipeThrough(nested()))
  console.log("nested", JSON.stringify(out))
}
main()
