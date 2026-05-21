# PowerShell recipes for pr-review

PowerShell-equivalent snippets for the stages that use `jq` and `comm` in the main SKILL.md. Read this when working on Windows without WSL or Git Bash, or when bash isn't available.

`ConvertFrom-Json` replaces `jq`. `Compare-Object` replaces `comm`. Stream redirection is `2>$null` instead of `2>/dev/null`.

## Setup

```powershell
$M = ".\target\release\mallard.exe"   # build first via `cargo build --release`
# If the release binary fails to load (`STATUS_DLL_NOT_FOUND`), fall back to:
# $M = "cargo"; $Mflags = @("run", "--quiet", "--", "query")
```

## Stage 1: PR metadata

```powershell
$pr = gh pr view $PR --json baseRefOid,headRefOid,files | ConvertFrom-Json
$BASE = $pr.baseRefOid
$HEAD = $pr.headRefOid
$CHANGED = $pr.files | ForEach-Object { $_.path } | Where-Object { $_ -like '*.rs' }
```

## Stage 3: added / removed symbols per file

```powershell
foreach ($f in $CHANGED) {
    $baseSyms = (& $M query symbols-in-file $f --index base.duckdb 2>$null | ConvertFrom-Json).value
    $headSyms = (& $M query symbols-in-file $f --index head.duckdb 2>$null | ConvertFrom-Json).value
    $baseIds  = $baseSyms | ForEach-Object { $_.id }
    $headIds  = $headSyms | ForEach-Object { $_.id }
    $added    = $headIds | Where-Object { $_ -notin $baseIds }
    $removed  = $baseIds | Where-Object { $_ -notin $headIds }
    $added   | ForEach-Object { "added $f $_" }
    $removed | ForEach-Object { "removed $f $_" }
}
```

## Stage 4: modified-body edge diff

For symbols present in both indexes (same ID), set-diff their outbound `calls` edge targets.

```powershell
foreach ($f in $CHANGED) {
    $baseSyms = (& $M query symbols-in-file $f --index base.duckdb 2>$null | ConvertFrom-Json).value
    $headSyms = (& $M query symbols-in-file $f --index head.duckdb 2>$null | ConvertFrom-Json).value
    $shared   = $baseSyms | Where-Object { $headSyms.id -contains $_.id }

    foreach ($s in $shared) {
        $baseE = (& $M query neighbors $s.id --kind calls --direction out --index base.duckdb 2>$null | ConvertFrom-Json).value
        $headE = (& $M query neighbors $s.id --kind calls --direction out --index head.duckdb 2>$null | ConvertFrom-Json).value
        $baseTargets = $baseE | ForEach-Object { if ($_.dst) { $_.dst.qualified_name } else { "[$($_.dst_unresolved)]" } } | Sort-Object -Unique
        $headTargets = $headE | ForEach-Object { if ($_.dst) { $_.dst.qualified_name } else { "[$($_.dst_unresolved)]" } } | Sort-Object -Unique
        $addedCalls   = $headTargets | Where-Object { $_ -notin $baseTargets }
        $removedCalls = $baseTargets | Where-Object { $_ -notin $headTargets }
        if ($addedCalls -or $removedCalls) {
            "modified-body $f $($s.qualified_name) id=$($s.id) added={$($addedCalls -join ',')} removed={$($removedCalls -join ',')}"
        }
    }
}
```

## Stage 5: gather evidence

```powershell
New-Item -ItemType Directory -Force evidence | Out-Null
foreach ($id in $changedIds) {
    & $M query expand $id --depth 1 --kind calls --direction both --index head.duckdb 2>$null | Set-Content "evidence/$id.expand.json"
    & $M query findings --symbol-id $id --index head.duckdb 2>$null | Set-Content "evidence/$id.findings.json"
}
```

## Notes

- PowerShell's `>` redirect operator captures only stdout by default — same as bash. Use `2>$null` to suppress stderr (cargo warnings, mallard tracing).
- `Set-Content` is preferred over `>` for capturing JSON to file; it sets UTF-8 by default in PowerShell 7+ which `ConvertFrom-Json` reads cleanly.
- `Sort-Object -Unique` is the `sort -u` equivalent.
- `Compare-Object` is `comm`'s analog when you want a single command, but the `Where-Object { $_ -notin $other }` pattern reads better for set difference.
