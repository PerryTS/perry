# Dot-source to install and configure the same pinned tools as wasi_toolchain.sh:
#   . ./scripts/wasi_toolchain.ps1
param([string]$Destination = '.cache/wasi')
& {
$ErrorActionPreference = 'Stop'

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$pins = switch ($architecture) {
    'X64' { @('x86_64', 'cccb5c323a9b34f0349a9b09e8804a0a7632c68c3310f4b5f437ed57d7e71d8f', 'd6cc742660e9edf35c92b47179808802f01eca3bf60ab4b0ac723c167592131f') }
    'Arm64' { @('arm64', '45e1c71f3e965621e7b98ebe1d37b0e4b1f77f3e8072113ffb4534e67b1a4b7c', 'ca945c51206ea7250ba1d2bd597f9e0d2363ee0b193cd54b783168d33310ec66') }
    default { throw "No pinned Windows WASI toolchain for $architecture" }
}
New-Item -ItemType Directory -Force $Destination | Out-Null
$destinationPath = (Resolve-Path -LiteralPath $Destination).Path
$sdkName = "wasi-sdk-34.0-$($pins[0])-windows"
$runtimeArch = if ($pins[0] -eq 'arm64') { 'aarch64' } else { $pins[0] }
$runtimeName = "wasmtime-v48.0.0-$runtimeArch-windows"

function Get-VerifiedArchive([string]$Url, [string]$Hash, [string]$FileName) {
    $archivePath = Join-Path $destinationPath $FileName
    if (!(Test-Path -LiteralPath $archivePath) -or
        (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Hash) {
        $partialPath = "$archivePath.part"
        & curl.exe -fsSL --retry 3 -o $partialPath $Url
        if ($LASTEXITCODE -ne 0) { throw "Download failed: $Url" }
        if ((Get-FileHash -LiteralPath $partialPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Hash) {
            throw "Checksum mismatch: $FileName"
        }
        Move-Item -LiteralPath $partialPath -Destination $archivePath -Force
    }
    return $archivePath
}

$sdkPath = Join-Path $destinationPath $sdkName
$runtimePath = Join-Path $destinationPath $runtimeName
if (!(Test-Path -LiteralPath (Join-Path $sdkPath 'bin/clang.exe'))) {
    $archive = Get-VerifiedArchive "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/$sdkName.tar.gz" $pins[1] "$sdkName.tar.gz"
    & tar -xzf $archive -C $destinationPath
    if ($LASTEXITCODE -ne 0) { throw 'wasi-sdk extraction failed' }
}
if (!(Test-Path -LiteralPath (Join-Path $runtimePath 'wasmtime.exe'))) {
    $archive = Get-VerifiedArchive "https://github.com/bytecodealliance/wasmtime/releases/download/v48.0.0/$runtimeName.zip" $pins[2] "$runtimeName.zip"
    Expand-Archive -LiteralPath $archive -DestinationPath $destinationPath -Force
}
if (!(Test-Path -LiteralPath (Join-Path $sdkPath 'share/wasi-sysroot')) -or
    !(Test-Path -LiteralPath (Join-Path $sdkPath 'bin/llvm-ar.exe')) -or
    !(Test-Path -LiteralPath (Join-Path $runtimePath 'wasmtime.exe'))) {
    throw 'WASI toolchain installation is incomplete'
}
$env:WASI_SDK_PATH = $sdkPath
$env:WASMTIME = Join-Path $runtimePath 'wasmtime.exe'
$env:CC_wasm32_wasip2 = Join-Path $sdkPath 'bin/clang.exe'
$env:AR_wasm32_wasip2 = Join-Path $sdkPath 'bin/llvm-ar.exe'
# cc-rs parses quoted flag arguments; preserve sysroots containing spaces.
$env:CFLAGS_wasm32_wasip2 = "--target=wasm32-wasip2 --sysroot=`"$(Join-Path $sdkPath 'share/wasi-sysroot')`""
$env:CC_SHELL_ESCAPED_FLAGS = '1'
Write-Host "WASI SDK: $sdkPath"
& $env:WASMTIME --version
if ($LASTEXITCODE -ne 0) { throw 'Wasmtime failed to start' }
}
