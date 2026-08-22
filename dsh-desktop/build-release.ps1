# Windows 入口：一键编译打包 DSH Desktop
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
node scripts/build-release.mjs @args
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
