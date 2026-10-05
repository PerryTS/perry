name: Setup Bun
description: Download, install, and setup Bun to your path.
author: robobun

branding:
  icon: play-circle
  color: white

inputs:
  bun-version:
    description: The version of Bun to install. (e.g. "latest", "canary", "1.0.0", "1.0.x", <sha>)
    required: false
  bun-version-file:
    description: The version of Bun to install from file. (e.g. "package.json", ".bun-version", ".tool-versions")
    default: null
    required: false
  bun-download-url:
    description: Override the URL to download Bun from. This skips version resolution and verifying AVX2 support.
    required: false
  registries:
    description: |
      List of package registries with authentication support. Format:
      - Default registry: https://registry.npmjs.org/
      - Default with token: https://registry.npmjs.org/|token
      - Scoped registry: @scope:https://registry.example.com/
      - Scoped with token: @scope:https://registry.example.com/|token
      - Scoped with credentials: @scope:https://user:pass@registry.example.com/
    required: false
  registry-url:
    required: false
    description: The URL of the package registry to use for installing Bun. Set the $BUN_AUTH_TOKEN environment variable to authenticate with the registry.
    deprecationMessage: "Use 'registries' input instead."
  scope:
    required: false
    description: "The scope for authenticating with the package registry."
    deprecationMessage: "Use 'registries' input instead."
  no-cache:
    required: false
    type: boolean
    default: false
    description: Disable caching of bun executable.
  token:
    required: false
    default: ${{ github.server_url == 'https://github.com' && github.token || '' }}
    description: Personal access token (PAT) used to fetch tags from oven-sh/bun repository. Recommended for resolving wildcard/range versions to avoid GitHub API rate limiting. When running this action on github.com, the default value is sufficient. When running on GHES, you can pass a personal access token for github.com if you are experiencing rate limiting.

outputs:
  bun-version:
    description: The version of Bun that was installed.
  bun-revision:
    description: The revision of Bun that was installed.
  bun-path:
    description: The path to the Bun executable.
  bun-download-url:
    description: The URL from which Bun was downloaded.
  cache-hit:
    description: If the version of Bun was cached.

runs:
  using: "node24"
  main: "dist/setup/index.js"
  post: "dist/cache-save/index.js"
  post-if: success()
