`class X extends TransformStream / ReadableStream / WritableStream` constructs
again: the live-heritage `super(...)` path (#12784) reached the WHATWG stream
base as a parent value it cannot construct and threw "Super constructor is not
a constructor"; the stream base's dedicated `super(...)` init is the
construction, so the live path stops there. A `TransformStream` subclass's
transformer `start(controller)` now runs at construction (eventsource-parser's
`EventSourceParserStream` builds its parser there; opencode's AI SDK response
stream failed every prompt with a 500).
