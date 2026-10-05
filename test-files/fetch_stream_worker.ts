import { parentPort, workerData } from 'node:worker_threads';
async function run() {
  let progress = true, matched = true;
  for (const path of ['worker', 'again']) {
    const response = await fetch(workerData.url + '/' + path + '?label=' + workerData.label);
    let count = 0, text = '';
    const decoder = new TextDecoder();
    for await (const chunk of response.body!) { count++; text += decoder.decode(chunk); }
    progress = progress && count > 1;
    matched = matched && JSON.parse(text).value === workerData.label;
  }
  parentPort!.postMessage({progress, matched});
}
await run();
