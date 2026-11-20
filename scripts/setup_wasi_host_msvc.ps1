# Initialize the installed Visual Studio tools without a Node action wrapper.
# Microsoft documents Launch-VsDevShell and the architecture/location switches:
# https://learn.microsoft.com/en-us/dotnet/framework/tools/developer-command-prompt-for-vs
$ErrorActionPreference = 'Stop'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $installation) {
    throw 'No Visual Studio installation with x64 C++ tools was found'
}
$launcher = Join-Path $installation.Trim() 'Common7/Tools/Launch-VsDevShell.ps1'
& $launcher -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
if ($env:VSCMD_ARG_TGT_ARCH -notin @('x64', 'amd64')) {
    throw "Unexpected Visual Studio target architecture: $env:VSCMD_ARG_TGT_ARCH"
}
foreach ($name in @('INCLUDE', 'LIB', 'LIBPATH', 'PATH', 'VCINSTALLDIR',
                   'VCToolsInstallDir', 'WindowsSdkDir', 'WindowsSDKVersion',
                   'VSCMD_ARG_TGT_ARCH', 'VSCMD_ARG_HOST_ARCH', 'VSINSTALLDIR')) {
    $value = [Environment]::GetEnvironmentVariable($name)
    if ($value) {
        "$name=$value" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
    }
}
foreach ($name in @('INCLUDE', 'LIB')) {
    if (-not [Environment]::GetEnvironmentVariable($name)) {
        throw "Visual Studio did not provide $name"
    }
}
Get-Command cl.exe, link.exe | Select-Object Name, Source
# Git Bash prepends its own /usr/bin, whose link.exe creates filesystem links.
# Cargo must use the selected MSVC linker regardless of shell PATH ordering.
$msvcLinker = Join-Path (Split-Path (Get-Command cl.exe).Source) 'link.exe'
if (-not (Test-Path -LiteralPath $msvcLinker)) {
    throw "Missing MSVC linker beside cl.exe: $msvcLinker"
}
"CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=$msvcLinker" |
    Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append

# Git Bash's Perl can lack modules required by vendored OpenSSL. Pin a
# validated native Windows Perl explicitly, independent of the next shell.
$perlCandidates = @('C:\Strawberry\perl\bin\perl.exe') +
    @(Get-Command perl.exe -All -ErrorAction SilentlyContinue | ForEach-Object { $_.Source })
$opensslPerl = $null
foreach ($candidate in ($perlCandidates | Select-Object -Unique)) {
    if (-not (Test-Path -LiteralPath $candidate)) { continue }
    & $candidate -MIPC::Cmd -MPod::Usage -MLocale::Maketext::Simple -e 'exit($^O eq "MSWin32" ? 0 : 1)'
    if ($LASTEXITCODE -eq 0) {
        $opensslPerl = $candidate
        break
    }
}
if (-not $opensslPerl) {
    throw 'No native Windows Perl with the required OpenSSL modules was found'
}
"OPENSSL_SRC_PERL=$opensslPerl" |
    Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
Write-Host "OpenSSL Perl: $opensslPerl"
