name: 'Deploy GitHub Pages site'
description: 'A GitHub Action to deploy an artifact as a GitHub Pages site'
author: 'GitHub'
runs:
  using: 'node24'
  main: 'dist/index.js'
inputs:
  token:
    description: 'GitHub token'
    default: ${{ github.token }}
    required: true
  timeout:
    description: 'Time in milliseconds after which to timeout and cancel the deployment (default: 10 minutes)'
    required: false
    default: '600000'
  error_count:
    description: 'Maximum number of status report errors before cancelling a deployment (default: 10)'
    required: false
    default: '10'
  reporting_interval:
    description: 'Initial time between deployment status reports; successful polls use capped backoff and jitter, with error backoff added separately (default: 5 seconds)'
    required: false
    default: '5000'
  artifact_name:
    description: 'Name of the artifact to deploy'
    required: false
    default: 'github-pages'
  preview:
    description: 'Is this attempting to deploy a pull request as a GitHub Pages preview site? (NOTE: This feature is only in alpha currently and is not available to the public!)'
    required: false
    default: 'false'
outputs:
  page_url:
    description: 'URL to deployed GitHub Pages'
