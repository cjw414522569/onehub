[CmdletBinding()]
param(
    [string]$SourceRoot
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($SourceRoot)) {
    $SourceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
}
$tempBase = [System.IO.Path]::GetTempPath()
$tempName = "ssh-t018-clean-$([guid]::NewGuid().ToString('N'))"
$tempRoot = Join-Path $tempBase $tempName
New-Item -ItemType Directory -Path $tempRoot | Out-Null

function Copy-Relative {
    param([string]$RelativePath)
    $source = Join-Path $SourceRoot $RelativePath
    $destination = Join-Path $tempRoot $RelativePath
    if (-not (Test-Path -LiteralPath $source)) { throw "Missing source fixture: $RelativePath" }
    $parent = Split-Path -Parent $destination
    if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
    Copy-Item -LiteralPath $source -Destination $destination -Recurse -Force
}

try {
    $files = @(
        '.gitattributes', '.gitignore', '.gitmessage', '.nvmrc', 'Cargo.toml', 'Cargo.lock',
        'global.json', 'rust-toolchain.toml', 'LICENSE', 'NOTICE'
    )
    $directories = @(
        '.github', '.githooks', 'architecture', 'apps', 'bindings', 'clients', 'crates',
        'docs', 'scripts', 'services', 'spikes', 'toolchains', 'tools'
    )
    foreach ($relative in $files + $directories) { Copy-Relative $relative }

    & git -C $tempRoot init --initial-branch=main | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'clean fixture git init failed' }
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $tempRoot 'scripts/init-git-governance.ps1') -RepositoryRoot $tempRoot
    if ($LASTEXITCODE -ne 0) { throw 'clean fixture governance initialization failed' }

    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $tempRoot 'scripts/bootstrap.ps1') -SkipPlatform
    if ($LASTEXITCODE -ne 0) { throw 'clean fixture bootstrap failed' }
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $tempRoot 'scripts/doctor.ps1') -SkipPlatform
    if ($LASTEXITCODE -ne 0) { throw 'clean fixture doctor failed' }

    Write-Host "Clean bootstrap fixture passed: $tempName"
} finally {
    if (Test-Path -LiteralPath $tempRoot) {
        $resolvedTemp = (Resolve-Path -LiteralPath $tempRoot).Path
        $resolvedBase = (Resolve-Path -LiteralPath $tempBase).Path.TrimEnd('\') + '\'
        if (-not $resolvedTemp.StartsWith($resolvedBase, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing cleanup outside temp base: $resolvedTemp"
        }
        Remove-Item -LiteralPath $resolvedTemp -Recurse -Force
    }
}
