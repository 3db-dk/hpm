#Requires -Version 5.0
<#
.SYNOPSIS
    Publish a built hpm binary as an asset on the GitHub release for the current tag.

.DESCRIPTION
    The Windows counterpart of ci/release-upload.sh, kept deliberately parallel to
    it. Python is not available on the Windows runner (see 9f5c867), so JSON is
    handled with ConvertTo-Json/ConvertFrom-Json rather than a shared script.

    Two Windows-specific hazards this guards against:

    1. try/catch does NOT catch a native executable's exit code. The previous
       implementation wrapped curl.exe in `try { ... } catch {}`, which is dead
       code -- curl.exe does not throw, it sets $LASTEXITCODE. Every curl.exe
       call below checks $LASTEXITCODE explicitly.
    2. An uncaught PowerShell error does not reliably set a non-zero process
       exit code, so the script ends in an explicit `exit 0` / `exit 1`.

    Reruns replace an existing asset rather than tolerating the 422 GitHub
    returns for a duplicate name; tolerating it kept the stale binary on the
    release while reporting success.
#>
param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Suffix
)

$ErrorActionPreference = 'Stop'

$repo = if ($env:RELEASE_REPO) { $env:RELEASE_REPO } else { '3db-dk/hpm' }
$bodyFile = Join-Path ([System.IO.Path]::GetTempPath()) "hpm-release-body-$PID.json"
$jsonFile = Join-Path ([System.IO.Path]::GetTempPath()) "hpm-release-$PID.json"

# Issues a GitHub API request, returns the HTTP status, leaves the body in
# $bodyFile. Deliberately omits -f so a 4xx reaches the status checks below.
function Invoke-Api {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][string]$Url,
        [string[]]$ExtraArgs = @()
    )
    $curlArgs = @(
        '-sS', '-X', $Method, $Url,
        '-H', "Authorization: token $env:GITHUB_TOKEN",
        '-H', 'Accept: application/vnd.github+json',
        '-o', $bodyFile,
        '-w', '%{http_code}'
    ) + $ExtraArgs

    $code = & curl.exe @curlArgs
    if ($LASTEXITCODE -ne 0) {
        throw "curl.exe exited $LASTEXITCODE for $Method $Url"
    }
    return "$code".Trim()
}

function Get-Body {
    if (Test-Path $bodyFile) { return (Get-Content $bodyFile -Raw) }
    return ''
}

# Parse the asset-list response in $bodyFile and return the single asset whose
# name matches, or $null. Assigns ConvertFrom-Json to a variable and wraps it
# in @() before filtering: under Windows PowerShell 5.1 an inline
# `ConvertFrom-Json | Where-Object` does NOT enumerate the array -- the whole
# array arrives as one $_, so the filter sees an object with no `.name` and
# passes the entire array through. Piping a variable enumerates correctly.
function Find-Asset {
    param([string]$Name)
    $assets = @(Get-Body | ConvertFrom-Json)
    return ($assets | Where-Object { $_.name -eq $Name } | Select-Object -First 1)
}

function Get-FailureMessage {
    param([string]$Message)
    return ($Message + "`n--- response body ---`n" + (Get-Body))
}

# Writes UTF-8 without a BOM; Set-Content -Encoding utf8 emits a BOM on
# Windows PowerShell 5.1, which GitHub rejects as malformed JSON.
function Write-Utf8NoBom {
    param([string]$Path, [string]$Content)
    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

try {
    if (-not $env:GITHUB_TOKEN) { throw 'GITHUB_TOKEN is not set' }
    if (-not $env:CI_COMMIT_TAG) { throw 'CI_COMMIT_TAG is not set' }

    $tag = $env:CI_COMMIT_TAG
    $version = $tag -replace '^v', ''
    $artifact = "hpm-v$version-$Suffix"

    if (-not (Test-Path $Source)) { throw "source binary not found: $Source" }

    New-Item -ItemType Directory -Force -Path artifacts | Out-Null
    $staged = Join-Path 'artifacts' $artifact
    Copy-Item $Source $staged -Force

    # --- release notes from CHANGELOG.md ------------------------------------

    $changelog = Get-Content CHANGELOG.md -Raw
    $pattern = "(?ms)^## \[$([regex]::Escape($version))\].*?`n(.*?)(?=^## \[|\z)"
    $match = [regex]::Match($changelog, $pattern)
    $notes = if ($match.Success) { $match.Groups[1].Value.Trim() } else { '' }

    $releaseData = @{ tag_name = $tag; name = $tag }
    if ($notes) { $releaseData.body = $notes } else { $releaseData.generate_release_notes = $true }
    Write-Utf8NoBom -Path $jsonFile -Content ($releaseData | ConvertTo-Json -Compress)

    # --- create the release --------------------------------------------------
    #
    # All three platform workflows race to create it: one gets 201, the losers
    # get 422. Any other status is a real fault.

    $code = Invoke-Api -Method 'POST' -Url "https://api.github.com/repos/$repo/releases" `
        -ExtraArgs @('-H', 'Content-Type: application/json', '--data-binary', "@$jsonFile")
    switch ($code) {
        '201' { Write-Host "created release $tag" }
        '422' { Write-Host "release $tag already exists" }
        default { throw (Get-FailureMessage "failed to create release (HTTP $code)") }
    }

    # --- resolve the release id ----------------------------------------------

    $code = Invoke-Api -Method 'GET' -Url "https://api.github.com/repos/$repo/releases/tags/$tag"
    if ($code -ne '200') { throw (Get-FailureMessage "failed to look up release $tag (HTTP $code)") }
    $releaseId = (Get-Body | ConvertFrom-Json).id
    if (-not $releaseId) { throw (Get-FailureMessage 'release lookup returned no id') }

    # --- drop any existing asset of the same name ----------------------------

    $assetsUrl = "https://api.github.com/repos/$repo/releases/$releaseId/assets?per_page=100"
    $code = Invoke-Api -Method 'GET' -Url $assetsUrl
    if ($code -ne '200') { throw (Get-FailureMessage "failed to list release assets (HTTP $code)") }
    $existing = Find-Asset -Name $artifact

    if ($existing) {
        $code = Invoke-Api -Method 'DELETE' `
            -Url "https://api.github.com/repos/$repo/releases/assets/$($existing.id)"
        if ($code -eq '204' -or $code -eq '404') {
            Write-Host "removed previous $artifact"
        }
        else {
            throw (Get-FailureMessage "failed to delete previous $artifact (HTTP $code)")
        }
    }

    # --- upload ---------------------------------------------------------------

    $uploadUrl = "https://uploads.github.com/repos/$repo/releases/$releaseId/assets?name=$artifact"
    $code = Invoke-Api -Method 'POST' -Url $uploadUrl `
        -ExtraArgs @('-H', 'Content-Type: application/octet-stream', '--data-binary', "@$staged")
    if ($code -ne '201') { throw (Get-FailureMessage "failed to upload $artifact (HTTP $code)") }

    # --- confirm what actually landed -----------------------------------------

    $expected = (Get-Item $staged).Length
    $code = Invoke-Api -Method 'GET' -Url $assetsUrl
    if ($code -ne '200') { throw (Get-FailureMessage "failed to verify release assets (HTTP $code)") }
    $asset = Find-Asset -Name $artifact

    if (-not $asset) {
        throw "$artifact is missing from the release after a successful upload"
    }
    if ($asset.state -ne 'uploaded') {
        throw "$artifact is in state '$($asset.state)', expected uploaded"
    }
    if ([long]$asset.size -ne [long]$expected) {
        throw "$artifact is $($asset.size) bytes on the release, expected $expected"
    }
    Write-Host "verified $artifact ($($asset.size) bytes)"
}
catch {
    Write-Host "release upload failed: $_" -ForegroundColor Red
    exit 1
}
finally {
    Remove-Item $bodyFile, $jsonFile -Force -ErrorAction SilentlyContinue
}

exit 0
