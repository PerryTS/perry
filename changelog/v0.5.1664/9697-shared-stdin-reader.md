Fixed `process.stdin` listener and keypress delivery across runtime and
`node:readline` APIs. Both surfaces now share one ordered fd-0 reader, so
`readline.emitKeypressEvents()` works with cooked and piped input and
`removeAllListeners([event])` reliably clears stdin listeners without racing or
dropping buffered bytes.
