[CmdletBinding()]
param(
    [string]$RulesFile,
    [switch]$RequireT006InProgress
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($RulesFile)) {
    $RulesFile = Join-Path $PSScriptRoot '..\architecture\dependency-rules.json'
}

function From-CodePoints {
    param([int[]]$CodePoints)
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

function Get-DependencyClosure {
    param([string]$StartId)
    $seen = @{}
    $queue = @($StartId)
    $cursor = 0
    while ($cursor -lt $queue.Count) {
        $current = [string]$queue[$cursor]
        $cursor++
        if ($seen.ContainsKey($current)) { continue }
        $seen[$current] = $true
        if (-not $script:modulesById.ContainsKey($current)) { continue }
        foreach ($dependency in @($script:modulesById[$current].dependencies)) {
            if (-not [string]::IsNullOrWhiteSpace([string]$dependency)) {
                $queue += [string]$dependency
            }
        }
    }
    return @($seen.Keys)
}

function Visit-Module {
    param([string]$ModuleId)
    if ($script:visitState[$ModuleId] -eq 1) {
        $script:cycleFound = $true
        return
    }
    if ($script:visitState[$ModuleId] -eq 2) { return }
    $script:visitState[$ModuleId] = 1
    foreach ($dependency in @($script:modulesById[$ModuleId].dependencies)) {
        Visit-Module -ModuleId ([string]$dependency)
    }
    $script:visitState[$ModuleId] = 2
}

$resolved = (Resolve-Path -LiteralPath $RulesFile).Path
$raw = Get-Content -Raw -Encoding UTF8 -LiteralPath $resolved
$errors = [System.Collections.Generic.List[string]]::new()
try {
    $model = $raw | ConvertFrom-Json
} catch {
    $errors.Add('Rules file is not valid JSON.')
    $model = $null
}

if ($null -ne $model) {
    if ([int]$model.schema_version -ne 1) { $errors.Add('schema_version must be 1.') }
    if ([string]$model.adr -ne 'ADR-002') { $errors.Add('adr must be ADR-002.') }
    foreach ($property in @('layer_order','layers','required_rules','modules','abi_contract')) {
        if ($null -eq $model.PSObject.Properties[$property]) {
            $errors.Add("Missing top-level property: $property")
        }
    }

    $layerValues = @{}
    if ($null -ne $model.layer_order) {
        foreach ($property in $model.layer_order.PSObject.Properties) {
            $layerValues[$property.Name] = [int]$property.Value
        }
    }
    $modules = @($model.modules)
    if ($modules.Count -lt 25) { $errors.Add("Expected at least 25 modules, found $($modules.Count).") }
    $script:modulesById = @{}
    $paths = @{}
    $requiredModuleIds = @('core-domain','core-protocol','session-orchestrator','ssh-backend','storage-sqlite','abi-c','clients-windows','clients-apple','clients-android','clients-linux','clients-web','cli','gateway','sync-service')
    foreach ($module in $modules) {
        $id = [string]$module.id
        if ([string]::IsNullOrWhiteSpace($id)) { $errors.Add('Module has no id.'); continue }
        if ($script:modulesById.ContainsKey($id)) { $errors.Add("Duplicate module id: $id"); continue }
        $script:modulesById[$id] = $module
        $layer = [string]$module.layer
        $pathValue = [string]$module.path
        if (-not $layerValues.ContainsKey($layer)) { $errors.Add("Module $id has unknown layer: $layer") }
        if ([string]::IsNullOrWhiteSpace($pathValue)) { $errors.Add("Module $id has no path.") }
        elseif ($paths.ContainsKey($pathValue)) { $errors.Add("Duplicate module path: $pathValue") }
        else { $paths[$pathValue] = $id }
        if ($null -eq $module.PSObject.Properties['dependencies']) { $errors.Add("Module $id has no dependencies field.") }
        if ($null -eq $module.PSObject.Properties['external_imports']) { $errors.Add("Module $id has no external_imports field.") }
    }
    foreach ($requiredId in $requiredModuleIds) {
        if (-not $script:modulesById.ContainsKey($requiredId)) { $errors.Add("Missing required module: $requiredId") }
    }

    foreach ($module in $modules) {
        $id = [string]$module.id
        if (-not $script:modulesById.ContainsKey($id)) { continue }
        $callerLayer = [string]$module.layer
        if (-not $layerValues.ContainsKey($callerLayer)) { continue }
        foreach ($dependency in @($module.dependencies)) {
            $dependencyId = [string]$dependency
            if (-not $script:modulesById.ContainsKey($dependencyId)) {
                $errors.Add("Module $id references unknown dependency: $dependencyId")
                continue
            }
            $calleeLayer = [string]$script:modulesById[$dependencyId].layer
            if ($layerValues[$callerLayer] -lt $layerValues[$calleeLayer]) {
                $errors.Add("Illegal dependency direction: $id ($callerLayer) -> $dependencyId ($calleeLayer)")
            }
        }
    }

    $script:visitState = @{}
    $script:cycleFound = $false
    foreach ($id in @($script:modulesById.Keys)) { Visit-Module -ModuleId $id }
    if ($script:cycleFound) { $errors.Add('Dependency graph contains a cycle.') }

    $requiredRules = $model.required_rules
    $uiModules = @($modules | Where-Object { ([string]$_.path) -match '^clients/' })
    $uiForbidden = @($requiredRules.ui_forbidden_modules)
    $uiBridgeIds = @('abi-c','wasm')
    foreach ($ui in $uiModules) {
        $closure = @(Get-DependencyClosure -StartId ([string]$ui.id))
        if (-not (($closure | Where-Object { $uiBridgeIds -contains [string]$_ }).Count -gt 0)) {
            $errors.Add("UI module has no ABI/WASM bridge: $($ui.id)")
        }
        foreach ($forbidden in $uiForbidden) {
            if ($closure -contains [string]$forbidden) {
                $errors.Add("UI module reaches forbidden implementation ${forbidden}: $($ui.id)")
            }
        }
    }

    $coreForbidden = @($requiredRules.core_forbidden_external_imports)
    foreach ($module in @($modules | Where-Object { @('L0','L1') -contains [string]$_.layer })) {
        foreach ($externalImport in @($module.external_imports)) {
            foreach ($forbiddenImport in $coreForbidden) {
                if ([string]$externalImport -like "*$forbiddenImport*") {
                    $errors.Add("Core module has forbidden external import ${forbiddenImport}: $($module.id)")
                }
            }
        }
    }

    $abiFields = @($model.abi_contract.message_fields)
    foreach ($requiredField in @($requiredRules.abi_required_message_fields)) {
        if ($abiFields -notcontains [string]$requiredField) {
            $errors.Add("ABI contract is missing required field: $requiredField")
        }
    }
    foreach ($requiredField in @('ownership','forbidden_cross_boundary_values')) {
        if ($null -eq $model.abi_contract.PSObject.Properties[$requiredField]) {
            $errors.Add("ABI contract is missing property: $requiredField")
        }
    }
}

if ($RequireT006InProgress) {
    $controlPath = Join-Path $PSScriptRoot '..\docs\PROJECT_CONTROL.md'
    $controlText = Get-Content -Raw -Encoding UTF8 -LiteralPath $controlPath
    $t006Line = ($controlText -split "`r?`n" | Where-Object { $_ -match '^\- \[[ x]\] T006 ' }) | Select-Object -First 1
    $inProgress = From-CodePoints @(0x72B6,0x6001,0xFF1A,0x8FDB,0x884C,0x4E2D)
    if (-not $t006Line -or -not $t006Line.Contains($inProgress)) {
        $errors.Add('T006 must be in progress before architecture validation runs.')
    }
}

if ($errors.Count -gt 0) {
    $errors | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "Architecture rules valid: $($modules.Count) modules, layer direction, cycle, UI bridge/forbidden reachability, core imports, ABI fields, and T006 status verified."