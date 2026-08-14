[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$reportPath = Join-Path $root 'docs\reports\WGPU_FEASIBILITY.json'
$markdownPath = Join-Path $root 'docs\reports\WGPU_FEASIBILITY.md'

& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'render-wgpu-feasibility-report.ps1') -ReportFile $reportPath -MarkdownFile $markdownPath
if ($LASTEXITCODE -ne 0) { throw "render-wgpu-feasibility-report.ps1 failed with exit code $LASTEXITCODE" }

$text = Get-Content -Raw -Encoding UTF8 -LiteralPath $markdownPath
if ($text -notmatch '(?m)^## Browser WebGPU evidence\r?$') { throw 'Rendered report is missing the Browser WebGPU evidence section.' }
if ($text -notmatch '(?m)^## Delivery phase\r?$') { throw 'Rendered report is missing the Delivery phase section.' }
if ($text -notmatch 'Delivery phase: \*\*Windows-first\*\*') { throw 'Rendered report is missing the Windows-first delivery phase statement.' }
if ($text -notmatch 'Current release platforms: windows') { throw 'Rendered report is missing the current Windows release platform statement.' }
if ($text -notmatch 'accepted native cells: 4') { throw 'Rendered report is missing the accepted Windows native cell count.' }
if ($text -notmatch 'Reserved future device classes:') { throw 'Rendered report is missing reserved future device classes.' }
if ($text -notmatch 'Interface reservation:') { throw 'Rendered report is missing interface reservation metadata.' }
if ($text -notmatch 'browser-webgpu-benchmark\.json') { throw 'Rendered report is missing the browser benchmark artifact reference.' }
if ($text -notmatch 'Chrome via CDP') { throw 'Rendered report is missing the browser runner identity.' }
if ($text -notmatch '(?m)^## Runner preflight\r?$') { throw 'Rendered report is missing the runner preflight section.' }
if ($text -notmatch 'runner-preflight\.json') { throw 'Rendered report is missing the runner preflight artifact reference.' }
if ($text -notmatch 'validate-runner-preflight\.mjs') { throw 'Rendered report is missing the runner preflight validator reference.' }
if ($text -notmatch 'native_metal') { throw 'Rendered report is missing the missing runner list.' }
if ($text -notmatch 'Host capabilities') { throw 'Rendered report is missing host capability evidence.' }
if ($text -notmatch 'docker=false') { throw 'Rendered report is missing container capability evidence.' }
if ($text -notmatch 'wsl=false') { throw 'Rendered report is missing WSL capability evidence.' }
if ($text -notmatch '(?m)^## Device-farm bundle\r?$') { throw 'Rendered report is missing the device-farm bundle section.' }
if ($text -notmatch 'device-farm-bundle\.json') { throw 'Rendered report is missing the device-farm bundle artifact reference.' }
if ($text -notmatch 'validate-wgpu-device-farm-bundle\.mjs') { throw 'Rendered report is missing the device-farm bundle validator reference.' }
if ($text -notmatch 'matrix_cells=56') { throw 'Rendered report is missing the device-farm bundle matrix count.' }
Write-Host 'wgpu report renderer contract passed'
