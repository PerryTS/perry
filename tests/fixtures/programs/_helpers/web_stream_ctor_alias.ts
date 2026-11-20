// Re-exports the node:stream/web constructors through const aliases, as
// @fails-components/webtransport's lib/webstreams.js does.
import {
  ReadableStream as ReadableStreamNode,
  WritableStream as WritableStreamNode,
} from "node:stream/web";

export const ReadableStream = ReadableStreamNode;
export const WritableStream = WritableStreamNode;
