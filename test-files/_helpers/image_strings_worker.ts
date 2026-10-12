import { parentPort } from 'node:worker_threads';
import { exercise } from './image_strings.ts';
parentPort!.postMessage(exercise());
