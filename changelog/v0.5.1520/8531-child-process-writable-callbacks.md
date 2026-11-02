### Fixed

- Child-process stdin writables now invoke completion callbacks for Node's
  `write` and `end` overloads on a later event-loop turn, preventing OpenCode
  LSP and formatter pipelines from stalling while they await a completed write.
