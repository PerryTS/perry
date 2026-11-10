// Issue #6117 repro (bug 1): connecting via wss:// panicked the tokio worker
// with "Could not automatically determine the process-level CryptoProvider"
// because no rustls provider was installed before the TLS handshake.
import { WebSocket } from 'ws'

async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

async function main() {
  const ws = new WebSocket('wss://echo.websocket.org')

  let opened = false
  let gotMessage = false

  ws.on('open', () => {
    opened = true
    console.log('Connected to WebSocket')
  })

  ws.on('message', (data) => {
    gotMessage = true
  })

  ws.on('error', (error) => {
    console.error('WebSocket error:', error)
  })

  let waits = 0
  while (ws.readyState !== 1 && waits < 100) {
    waits++
    await sleep(100)
  }

  if (ws.readyState === 1) {
    console.log('readyState reached OPEN')
    ws.send('Hello again!')
    await sleep(1500)
    console.log('received message after send:', gotMessage)
    ws.close()
    await sleep(500)
    console.log('readyState after close is CLOSING/CLOSED:', ws.readyState === 2 || ws.readyState === 3)
  } else {
    console.log('FAILED: never reached OPEN after', waits, 'waits')
  }
}

await main()
