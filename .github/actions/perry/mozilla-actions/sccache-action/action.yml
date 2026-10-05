---
name: "Sccache Action"
description: "Setup sccache action"
author: "mozilla"
inputs:
  version:
    description: "The installed sccache version. Use the latest version of sccache if not specified."
  token:
    description: "Used to pull the latest release from sccache. When running this action outside of github.com, you have to pass a personal access token for github.com."
    default: ${{ github.server_url == 'https://github.com' && github.token || '' }}
  disable_annotations:
    description: "Disable annotation generation in post-run."
    default: 'false'
runs:
  using: "node24"
  main: "dist/setup/index.js"
  post: "dist/show_stats/index.js"
branding:
  icon: "star"
  color: "orange"
