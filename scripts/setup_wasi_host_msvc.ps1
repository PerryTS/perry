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
