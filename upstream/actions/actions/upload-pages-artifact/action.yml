name: "Upload GitHub Pages artifact"
description: "A composite action that prepares your static assets to be deployed to GitHub Pages"
author: "GitHub"
inputs:
  name:
    description: 'Artifact name'
    required: false
    default: 'github-pages'
  path:
    description: "Path of the directory containing the static assets."
    required: true
    default: "_site/"
  retention-days:
    description: "Duration after which artifact will expire in days."
    required: false
    default: "1"
  include-hidden-files:
    description: "Include hidden files and directories (those starting with a dot) in the artifact. Excludes .git and .github regardless."
    required: false
    default: "false"
outputs:
  artifact_id:
    description: "The ID of the artifact that was uploaded."
    value: ${{ steps.upload-artifact.outputs.artifact-id }}
runs:
  using: composite
  steps:
    - name: Archive artifact
      shell: sh
      if: runner.os == 'Linux'
      run: |
        echo ::group::Archive artifact
        tar \
          --dereference --hard-dereference \
          --directory "$INPUT_PATH" \
          -cvf "$RUNNER_TEMP/artifact.tar" \
          --exclude=.git \
          --exclude=.github \
          ${{ inputs.include-hidden-files != 'true' && '--exclude=.[^/]*' || '' }} \
          .
        echo ::endgroup::
      env:
        INPUT_PATH: ${{ inputs.path }}

    # Switch to gtar (GNU tar instead of bsdtar which is the default in the MacOS runners so we can use --hard-dereference)
    - name: Archive artifact
      shell: sh
      if: runner.os == 'macOS'
      run: |
        echo ::group::Archive artifact
        gtar \
          --dereference --hard-dereference \
          --directory "$INPUT_PATH" \
          -cvf "$RUNNER_TEMP/artifact.tar" \
          --exclude=.git \
          --exclude=.github \
          ${{ inputs.include-hidden-files != 'true' && '--exclude=.[^/]*' || '' }} \
          .
        echo ::endgroup::
      env:
        INPUT_PATH: ${{ inputs.path }}

    # Massage the paths for Windows only
    - name: Archive artifact
      shell: bash
      if: runner.os == 'Windows'
      run: |
        echo ::group::Archive artifact
        tar \
          --dereference --hard-dereference \
          --directory "$INPUT_PATH" \
          -cvf "$RUNNER_TEMP\artifact.tar" \
          --exclude=.git \
          --exclude=.github \
          ${{ inputs.include-hidden-files != 'true' && '--exclude=.[^/]*' || '' }} \
          --force-local \
          "."
        echo ::endgroup::
      env:
        INPUT_PATH: ${{ inputs.path }}

    - name: Upload artifact
      id: upload-artifact
      uses: actions/upload-artifact@bbbca2ddaa5d8feaa63e36b76fdaad77386f024f # v7.0.0
      with:
        name: ${{ inputs.name }}
        path: ${{ runner.temp }}/artifact.tar
        retention-days: ${{ inputs.retention-days }}
        if-no-files-found: error
