param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\qqcli"
)

$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot 'qq.exe'
$versionSource = Join-Path $PSScriptRoot 'VERSION.txt'

if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
    throw "安装包中未找到 qq.exe：$source"
}
if (-not (Test-Path -LiteralPath $versionSource -PathType Leaf)) {
    throw "安装包中未找到 VERSION.txt：$versionSource"
}

$expectedVersion = (Get-Content -LiteralPath $versionSource -Raw).Trim()
$reportedVersion = (& $source --version 2>$null | Out-String).Trim()
if ($reportedVersion -notmatch "^qq\s+$([regex]::Escape($expectedVersion))$") {
    throw "版本校验失败：VERSION.txt=$expectedVersion，qq.exe=$reportedVersion"
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Get-ChildItem -LiteralPath $PSScriptRoot -File |
    Where-Object { $_.Extension -in @('.exe', '.dll') } |
    Copy-Item -Destination $InstallDir -Force
Copy-Item -LiteralPath $versionSource -Destination (Join-Path $InstallDir 'VERSION.txt') -Force
foreach ($doc in @('README.md', 'README_CN.md', 'RELEASE-MANIFEST.json')) {
    $docPath = Join-Path $PSScriptRoot $doc
    if (Test-Path -LiteralPath $docPath -PathType Leaf) {
        Copy-Item -LiteralPath $docPath -Destination (Join-Path $InstallDir $doc) -Force
    }
}

$installedVersion = (& (Join-Path $InstallDir 'qq.exe') --version 2>$null | Out-String).Trim()
if ($installedVersion -notmatch "^qq\s+$([regex]::Escape($expectedVersion))$") {
    throw "安装后版本校验失败：$installedVersion"
}

$currentPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$pathEntries = @($currentPath -split ';' | Where-Object { $_ })
if ($pathEntries -notcontains $InstallDir) {
    $newPath = (@($pathEntries) + $InstallDir) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
}

Write-Host "qqcli 已安装到：$InstallDir"
Write-Host '请重新打开 PowerShell，然后依次运行：'
Write-Host '  qq init'
Write-Host '  qq doctor'
