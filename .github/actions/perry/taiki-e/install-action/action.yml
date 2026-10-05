name: Install development tools
description: GitHub Action for installing development tools

inputs:
  tool:
    description: Tools to install (whitespace or comma separated list)
    required: true
    # default: #publish:tool
  checksum:
    description: Whether to enable checksums (strongly discouraged to disable)
    required: false
    default: 'true'
  fallback:
    description: Whether to use fallback (none, cargo-binstall, or cargo-install)
    required: false
    default: 'cargo-binstall'

# Note:
# - inputs.* should be manually mapped to INPUT_* due to https://github.com/actions/runner/issues/665
# - For context containing path, use GITHUB_*/RUNNER_* instead of github.*/runner.* due to https://github.com/actions/runner/issues/2185
runs:
  using: composite
  steps:
    - run: |
        bail() {
          printf '::error::install-action: %s\n' "$*"
          exit 1
        }
        # If /bin/sh is dash, environment variable containing % is not imported, but is fine
        # because it also means that it will not be exposed to subprocess.
        if /usr/bin/env | grep -Eq '^BASH_FUNC_'; then
          bail 'bash function injection via BASH_FUNC_ environment variable is not allowed for security reasons'
        fi
        if ! command -v bash >/dev/null; then
          if grep -Eq '^ID=alpine' /etc/os-release; then
            printf '::group::Install packages required for install-action (bash)\n'
            # NB: sync with apk_install in main.sh
            if command -v sudo >/dev/null; then
              sudo apk --no-cache add bash
            elif command -v doas >/dev/null; then
              doas apk --no-cache add bash
            else
              apk --no-cache add bash
            fi
            printf '::endgroup::\n'
          else
            bail 'this action requires bash'
          fi
        fi
        bash --noprofile --norc "${GITHUB_ACTION_PATH:?}/main.sh"
      shell: /usr/bin/env -u ENV -u BASH_ENV -u CDPATH -u SHELLOPTS -u BASHOPTS /bin/sh -eu {0} # zizmor: ignore[misfeature] false positive
      env:
        # NB: Sync with Windows case.
        INPUT_TOOL: ${{ inputs.tool }}
        INPUT_CHECKSUM: ${{ inputs.checksum }}
        INPUT_FALLBACK: ${{ inputs.fallback }}
        DEFAULT_GITHUB_TOKEN: ${{ inputs.fallback == 'cargo-binstall' && github.token || '' }}
        ACTION_USER_AGENT: ${{ github.action_repository }} (${{ github.action_ref }})
        RUNNER_OS: ${{ runner.os }}
        RUNNER_ARCH: ${{ runner.arch }}
      if: runner.os != 'Windows'
    # Use pwsh and retry on bash startup failure to work around windows-11-arm runner bug:
    # https://github.com/actions/partner-runner-images/issues/169
    - run: |
        Set-StrictMode -Version Latest
        $remove_env = @('ENV','BASH_ENV','CDPATH','SHELLOPTS','BASHOPTS','BASH_FUNC_*')
        foreach ($name in $remove_env) {
          if (Test-Path "Env:$name") { Remove-Item "Env:\$name" }
        }
        for ($i=1; $i -le 10; $i++) {
          $prev_err_action = $ErrorActionPreference
          $ErrorActionPreference = "Continue"
          & bash --noprofile --norc "$env:GITHUB_ACTION_PATH\main.sh"
          $code = $LASTEXITCODE
          $ErrorActionPreference = "$prev_err_action"
          if (Test-Path "$env:USERPROFILE\.install-action\init") {
            # If bash started successfully, main.sh creates init file.
            Remove-Item "$env:USERPROFILE\.install-action\init" -Force
            exit $code
          }
          if ($i -lt 10) {
            Write-Output "::warning::install-action: installation failed due to bash startup failure (<https://github.com/actions/partner-runner-images/issues/169>); retrying..."
          }
        }
        Write-Output "::error::install-action: installation failed due to bash startup failure (<https://github.com/actions/partner-runner-images/issues/169>); this maybe resolved by re-running job"
        exit 1
      shell: pwsh
      env:
        # NB: Sync with non-Windows case.
        INPUT_TOOL: ${{ inputs.tool }}
        INPUT_CHECKSUM: ${{ inputs.checksum }}
        INPUT_FALLBACK: ${{ inputs.fallback }}
        DEFAULT_GITHUB_TOKEN: ${{ inputs.fallback == 'cargo-binstall' && github.token || '' }}
        ACTION_USER_AGENT: ${{ github.action_repository }} (${{ github.action_ref }})
        RUNNER_OS: ${{ runner.os }}
        RUNNER_ARCH: ${{ runner.arch }}
      if: runner.os == 'Windows'
