[CmdletBinding()]
param(
    [string]$OutputFile,
    [string]$ManifestFile
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputFile)) {
    $OutputFile = Join-Path $root 'spikes\wgpu-terminal\fixtures\replay-10mb.bin'
}
if ([string]::IsNullOrWhiteSpace($ManifestFile)) {
    $ManifestFile = Join-Path $root 'spikes\wgpu-terminal\fixtures\manifest.json'
}

$outputPath = [System.IO.Path]::GetFullPath($OutputFile)
$manifestPath = [System.IO.Path]::GetFullPath($ManifestFile)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $outputPath) | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $manifestPath) | Out-Null

$esc = [char]27
$bel = [char]7
$seed = @(
    "$esc[2J$esc[H$esc]0;wgpu-terminal$bel`r`n",
    "user@host:~$ printf 'Ligature: office affinity -> fi ffi; CJK: 界面; emoji: 🙂'`r`n",
    "Ligature: office affinity -> fi ffi; CJK: 界面; emoji: 🙂`r`n",
    "$esc[31mred$esc[0m $esc[1;34mbold blue$esc[0m`r`n",
    "IME composition: n -> ni -> 你 -> 你好`r`n",
    "scrollback row 0000000001`r`n"
)
$encoding = [System.Text.UTF8Encoding]::new($false)
$seedBytes = [System.Collections.Generic.List[byte]]::new()
foreach ($line in $seed) {
    $seedBytes.AddRange($encoding.GetBytes($line))
}
$seedBytes.Add(0xff)
$seedBytes.AddRange([byte[]](0xc3, 0x28))

$targetBytes = 10 * 1024 * 1024
$buffer = [byte[]]::new($targetBytes)
for ($index = 0; $index -lt $targetBytes; $index++) {
    $buffer[$index] = $seedBytes[$index % $seedBytes.Count]
}
[System.IO.File]::WriteAllBytes($outputPath, $buffer)
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $outputPath).Hash.ToLowerInvariant()

$relativeFile = $outputPath.Substring($root.Length).TrimStart([char]92, [char]47)
$manifest = [ordered]@{
    schema_version = 1
    fixture_id = 'wgpu-terminal-replay-10mb-v1'
    file = $relativeFile
    bytes = $targetBytes
    sha256 = $hash
    seed = 'replay-seed.txt plus CSI/OSC/SGR/CRLF/CJK/emoji/invalid-UTF8 markers'
    generated_at_utc = [DateTime]::UtcNow.ToString('o')
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath $manifestPath
Write-Host "Generated wgpu fixture: file=$outputPath bytes=$targetBytes sha256=$hash"
