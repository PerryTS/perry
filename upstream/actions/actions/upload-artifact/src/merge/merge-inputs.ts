export interface MergeInputs {
  /**
   * The name of the artifact that the artifacts will be merged into
   */
  name: string

  /**
   * A glob pattern matching the artifacts that should be merged.
   */
  pattern: string

  /**
   * Duration after which artifact will expire in days
   */
  retentionDays: number

  /**
   * The level of compression for Zlib to be applied to the artifact archive.
   */
  compressionLevel?: number

  /**
   * If true, the artifacts that were merged will be deleted.
   * If false, the artifacts will still exist.
   */
  deleteMerged: boolean

  /**
   * If true, the artifacts will be merged into separate directories.
   * If false, the artifacts will be merged into the root of the destination.
   */
  separateDirectories: boolean

  /**
   * Whether or not to include hidden files in the artifact
   */
  includeHiddenFiles: boolean
}
