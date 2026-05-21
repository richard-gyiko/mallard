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

Uses the bulk `edges-by-file` primitive — one query per file per direction. For each shared-ID symbol, set-diff base vs head `outbound`.

```powershell
foreach ($f in $CHANGED) {
    $baseBundles = (& $M query edges-by-file $f --kind calls --direction out --index base.duckdb 2>$null | ConvertFrom-Json).value
    $headBundles = (& $M query edges-by-file $f --kind calls --direction out --index head.duckdb 2>$null | ConvertFrom-Json).value
    $baseById = @{}
    $baseBundles | ForEach-Object { $baseById[$_.symbol.id] = $_ }

    foreach ($hb in $headBundles) {
        if (-not $baseById.ContainsKey($hb.symbol.id)) { continue }   # added in head only -> stage 3 saw it
        $bb = $baseById[$hb.symbol.id]
        $baseTargets = @($bb.outbound | ForEach-Object { if ($_.dst) { $_.dst.qualified_name } else { "[$($_.dst_unresolved)]" } } | Sort-Object -Unique)
        $headTargets = @($hb.outbound | ForEach-Object { if ($_.dst) { $_.dst.qualified_name } else { "[$($_.dst_unresolved)]" } } | Sort-Object -Unique)
        $addedCalls   = @($headTargets | Where-Object { $_ -notin $baseTargets })
        $removedCalls = @($baseTargets | Where-Object { $_ -notin $headTargets })
        if ($addedCalls.Count -gt 0 -or $removedCalls.Count -gt 0) {
            "modified-body $f $($hb.symbol.qualified_name) id=$($hb.symbol.id) added={$($addedCalls -join ',')} removed={$($removedCalls -join ',')}"
        }
    }
}
```

Verified on PR #7 (4 files, ~60 stable symbols): **8.1 seconds wall clock** with the new primitive vs ~5 minutes with the per-symbol `neighbors` approach.

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
