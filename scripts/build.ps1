param([string]$Dotnet = 'dotnet')
$ErrorActionPreference = 'Stop'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_GENERATE_ASPNET_CERTIFICATE = 'false'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    & $Dotnet run --project tests/CodexVoiceClipboard.Tests -c Release
    if ($LASTEXITCODE -ne 0) { throw 'Core tests failed.' }
    & $Dotnet publish src/CodexVoiceClipboard -c Release -o artifacts/publish
    if ($LASTEXITCODE -ne 0) { throw 'Publish failed.' }
    Get-FileHash artifacts/publish/CodexVoiceClipboard.exe -Algorithm SHA256
} finally { Pop-Location }
