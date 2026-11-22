# Runs the Verus proofs of genie. See VERIFICATION.md.
# With -Required, the script fails when Verus is not on the computer.
param([switch]$Required)

$crates = @('genie-core', 'genie-formats')

function Test-Verus {
    $null -ne (Get-Command cargo-verus -ErrorAction SilentlyContinue)
}

if (-not (Test-Verus)) {
    $folders = @($env:GENIE_VERUS)
    $tools = Join-Path (Join-Path $HOME 'tools') 'verus'
    if (Test-Path $tools) {
        $folders += Get-ChildItem $tools -Directory -Filter 'verus-*' | ForEach-Object { $_.FullName }
    }
    foreach ($folder in $folders | Where-Object { $_ }) {
        $names = @('cargo-verus.exe', 'cargo-verus')
        if ($names | Where-Object { Test-Path (Join-Path $folder $_) }) {
            $env:PATH = $folder + [System.IO.Path]::PathSeparator + $env:PATH
            break
        }
    }
}

if (-not (Test-Verus)) {
    Write-Output 'Verus is not on this computer. The proofs did not run.'
    if ($Required) {
        exit 1
    }
    exit 0
}

foreach ($crate in $crates) {
    cargo verus verify --package $crate
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
