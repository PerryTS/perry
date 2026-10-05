name: 'Upload a Build Artifact'
description: 'Upload a build artifact that can be used by subsequent workflow steps'
author: 'GitHub'
inputs:
  name:
    description: 'Artifact name. If the `archive` input is `false`, the name of the file uploaded will be the artifact name.'
    default: 'artifact'
  path:
    description: 'A file, directory or wildcard pattern that describes what to upload.'
    required: true
  if-no-files-found:
    description: >
      The desired behavior if no files are found using the provided path.

      Available Options:
        warn: Output a warning but do not fail the action
        error: Fail the action with an error message
        ignore: Do not output any warnings or errors, the action does not fail
    default: 'warn'
  retention-days:
    description: >
      Duration after which artifact will expire in days. 0 means using default retention.

      Minimum 1 day.
      Maximum 90 days unless changed from the repository settings page.
  compression-level:
    description: >
      The level of compression for Zlib to be applied to the artifact archive.
      The value can range from 0 to 9:
      - 0: No compression
      - 1: Best speed
      - 6: Default compression (same as GNU Gzip)
      - 9: Best compression
      Higher levels will result in better compression, but will take longer to complete.
      For large files that are not easily compressed, a value of 0 is recommended for significantly faster uploads.
    default: '6'
  overwrite:
    description: >
      If true, an artifact with a matching name will be deleted before a new one is uploaded.
      If false, the action will fail if an artifact for the given name already exists.
      Does not fail if the artifact does not exist.
    default: 'false'
  include-hidden-files:
    description: >
      If true, hidden files will be included in the artifact.
      If false, hidden files will be excluded from the artifact.
    default: 'false'
  archive:
    description: >
      If true, the artifact will be archived (zipped) before uploading.
      If false, the artifact will be uploaded as-is without archiving.
      When `archive` is `false`, only a single file can be uploaded. The name of the file will be used as the artifact name (ignoring the `name` parameter).
    default: 'true'

outputs:
  artifact-id:
    description: >
      A unique identifier for the artifact that was just uploaded. Empty if the artifact upload failed.

      This ID can be used as input to other APIs to download, delete or get more information about an artifact: https://docs.github.com/en/rest/actions/artifacts
  artifact-url:
    description: >
      A download URL for the artifact that was just uploaded. Empty if the artifact upload failed.

      This download URL only works for requests Authenticated with GitHub. Anonymous downloads will be prompted to first login. 
      If an anonymous download URL is needed than a short time restricted URL can be generated using the download artifact API: https://docs.github.com/en/rest/actions/artifacts#download-an-artifact    

      This URL will be valid for as long as the artifact exists and the workflow run and repository exists. Once an artifact has expired this URL will no longer work.
      Common uses cases for such a download URL can be adding download links to artifacts in descriptions or comments on pull requests or issues.
  artifact-digest:
    description: >
      SHA-256 digest for the artifact that was just uploaded. Empty if the artifact upload failed.
runs:
  using: 'node24'
  main: 'dist/upload/index.js'
