name: 'Configure GitHub Pages'
description: 'A GitHub Action to enable Pages, extract various metadata about a site, and configure some supported static site generators.'
author: 'GitHub'
runs:
  using: 'node24'
  main: 'dist/index.js'
inputs:
  static_site_generator:
    description: 'Optional static site generator to attempt to configure: "nuxt", "next", "gatsby", or "sveltekit"'
    required: false
  generator_config_file:
    description: 'Optional file path to static site generator configuration file'
    required: false
  token:
    description: 'GitHub token'
    default: ${{ github.token }}
    required: true
  enablement:
    description: 'Try to enable Pages for the repository if it is not already enabled. This option requires a token other than `GITHUB_TOKEN` to be provided. In the context of a Personal Access Token, the `repo` scope or Pages write permission is required. In the context of a GitHub App, the `administration:write` and `pages:write` permissions are required.'
    default: 'false'
    required: false
outputs:
  base_url:
    description: 'GitHub Pages site full base URL. Examples: "https://octocat.github.io/my-repo", "https://octocat.github.io", "https://www.example.com"'
  origin:
    description: 'GitHub Pages site origin. Examples: "https://octocat.github.io", "https://www.example.com"'
  host:
    description: 'GitHub Pages site host. Examples: "octocat.github.io", "www.example.com"'
  base_path:
    description: 'GitHub Pages site full base path. Examples: "/my-repo" or ""'
