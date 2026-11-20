// Issue #6117 repro (bug 2): WebSocket static readyState constants from 'ws'
// must be readable as values — `WebSocket.OPEN` etc. crashed with
// "TypeError: Cannot read properties of undefined (reading 'OPEN')".
import { WebSocket } from 'ws'

console.log('CONNECTING =', WebSocket.CONNECTING)
console.log('OPEN =', WebSocket.OPEN)
console.log('CLOSING =', WebSocket.CLOSING)
console.log('CLOSED =', WebSocket.CLOSED)

// The exact comparison shape from the issue: instance readyState vs static.
const ws = new WebSocket('ws://127.0.0.1:9')

let waits = 0
async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

async function main() {
  while (ws.readyState !== WebSocket.OPEN && waits < 3) {
    console.log('Waiting for connection...', ws.readyState === WebSocket.CONNECTING ? 'still connecting' : 'not connecting')
    waits++
    await sleep(50)
  }
  console.log('done waiting, readyState is number:', typeof ws.readyState === 'number')
  // Connection to a closed port must end CLOSED (3), not stuck CONNECTING.
  await sleep(200)
  console.log('refused connect ends CLOSED:', ws.readyState === WebSocket.CLOSED)
  ws.close()
}

await main()
