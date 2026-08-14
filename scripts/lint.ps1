[CmdletBinding()]
param([switch]$Ci, [switch]$SkipPlatform)
$dispatcher = Join-Path $PSScriptRoot 'dev.ps1'
$arguments = @('-Command', 'lint')
if ($Ci) { $arguments += '-Ci' }
if ($SkipPlatform) { $arguments += '-SkipPlatform' }
& powershell -NoProfile -ExecutionPolicy Bypass -File $dispatcher @arguments
exit $LASTEXITCODE
