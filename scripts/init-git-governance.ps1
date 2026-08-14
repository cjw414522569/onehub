[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Get-Location).Path
)

$resolvedRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw 'git executable is required'
}

$required = @(
    '.gitignore', '.gitattributes', '.gitmessage', '.githooks/commit-msg', '.githooks/pre-push',
    '.github/CODEOWNERS', '.github/branch-protection.yml', '.github/pull_request_template.md',
    '.github/ISSUE_TEMPLATE/bug_report.yml', '.github/ISSUE_TEMPLATE/feature_request.yml',
    '.github/workflows/ci.yml', 'scripts/validate-commit-message.mjs', 'scripts/validate-git-governance.mjs'
)
foreach ($relative in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $resolvedRoot $relative) -PathType Leaf)) {
        throw "missing governance artifact: $relative"
    }
}

& git -C $resolvedRoot init --initial-branch=main 2>$null
if ($LASTEXITCODE -ne 0) {
    & git -C $resolvedRoot init
    if ($LASTEXITCODE -ne 0) { throw 'git init failed' }
    & git -C $resolvedRoot branch -M main
    if ($LASTEXITCODE -ne 0) { throw 'unable to set initial branch to main' }
}

$settings = @(
    @('core.hooksPath', '.githooks'),
    @('commit.template', '.gitmessage'),
    @('init.defaultBranch', 'main'),
    @('pull.ff', 'only'),
    @('fetch.prune', 'true'),
    @('rerere.enabled', 'true')
)
foreach ($setting in $settings) {
    & git -C $resolvedRoot config $setting[0] $setting[1]
    if ($LASTEXITCODE -ne 0) { throw "git config failed: $($setting[0])" }
}

Write-Host "Git governance initialized: root=$resolvedRoot branch=main hooks=.githooks commit_template=.gitmessage"

