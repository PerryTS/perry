export enum Inputs {
  Name = 'name',
  Path = 'path',
  GitHubToken = 'github-token',
  Repository = 'repository',
  RunID = 'run-id',
  Pattern = 'pattern',
  MergeMultiple = 'merge-multiple',
  ArtifactIds = 'artifact-ids',
  SkipDecompress = 'skip-decompress',
  DigestMismatch = 'digest-mismatch'
}

export enum DigestMismatchBehavior {
  Ignore = 'ignore',
  Info = 'info',
  Warn = 'warn',
  Error = 'error'
}

export enum Outputs {
  DownloadPath = 'download-path'
}
