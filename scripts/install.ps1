param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\qqcli"
)

$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot 'qq.exe'

if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
    throw "安装包中未找到 qq.exe：$source"
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -LiteralPath $source -Destination (Join-Path $InstallDir 'qq.exe') -Force

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
