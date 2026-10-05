name: 'Download a Build Artifact'
description: 'Download a build artifact that was previously uploaded in the workflow by the upload-artifact action'
author: 'GitHub'
inputs:
  name:
    description: 'Name of the artifact to download. If unspecified, all artifacts for the run are downloaded.'
    required: false
  artifact-ids:
    description: 'IDs of the artifacts to download, comma-separated. Either inputs `artifact-ids` or `name` can be used, but not both.'
    required: false
  path:
    description: 'Destination path. Supports basic tilde expansion. Defaults to $GITHUB_WORKSPACE'
    required: false
  pattern:
    description: 'A glob pattern matching the artifacts that should be downloaded. Ignored if name is specified.'
    required: false
  merge-multiple:
    description: 'When multiple artifacts are matched, this changes the behavior of the destination directories.
      If true, the downloaded artifacts will be in the same directory specified by path.
      If false, the downloaded artifacts will be extracted into individual named directories within the specified path.'
    required: false
    default: 'false'
  github-token:
    description: 'The GitHub token used to authenticate with the GitHub API.
      This is required when downloading artifacts from a different repository or from a different workflow run.
      If this is not specified, the action will attempt to download artifacts from the current repository and the current workflow run.'
    required: false
  repository:
    description: 'The repository owner and the repository name joined together by "/".
      If github-token is specified, this is the repository that artifacts will be downloaded from.'
    required: false
    default: ${{ github.repository }}
  run-id:
    description: 'The id of the workflow run where the desired download artifact was uploaded from.
      If github-token is specified, this is the run that artifacts will be downloaded from.'
    required: false
    default: ${{ github.run_id }}
  skip-decompress:
    description: 'If true, the downloaded artifact will not be automatically extracted/decompressed.
      This is useful when you want to handle the artifact as-is without extraction.'
    required: false
    default: 'false'
  digest-mismatch:
    description: 'The behavior when a downloaded artifact''s digest does not match the expected digest.
      Options: ignore, info, warn, error. Default is error which will fail the action.'
    required: false
    default: 'error'
outputs:
  download-path:
    description: 'Path of artifact download'
runs:
  using: 'node24'
  main: 'dist/index.js'
