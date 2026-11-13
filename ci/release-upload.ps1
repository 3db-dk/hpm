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

    Beside the binary it publishes "<asset>.sha256" ("<hex>  <asset>", the
    format `sha256sum -c` reads), uploaded first. A published binary is never
    replaced: a rerun whose binary matches the published checksum is a no-op,
    and a different binary, or a published binary without a checksum file, is
    an error (new bytes need a new tag). Same behaviour as release-upload.sh,
    whose tests (ci/tests/release_upload_test.sh) define it.
#>
param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Suffix
)

$ErrorActionPreference = 'Stop'

$repo = if ($env:RELEASE_REPO) { $env:RELEASE_REPO } else { '3db-dk/hpm' }
# The token is sent to these, so they can only be GitHub or this machine.
$api = if ($env:GITHUB_API_URL) { $env:GITHUB_API_URL } else { 'https://api.github.com' }
$uploads = if ($env:GITHUB_UPLOADS_URL) { $env:GITHUB_UPLOADS_URL } else { 'https://uploads.github.com' }
foreach ($base in @($api, $uploads)) {
    if ($base -notmatch '^(https://(api|uploads)\.github\.com|http://127\.0\.0\.1:[0-9]+)$') {
        Write-Host "refusing API base ${base}: only GitHub or 127.0.0.1" -ForegroundColor Red
        exit 1
    }
}
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
# name matches, or $null.
#
# Iterated with foreach rather than a `| Where-Object | Select-Object` pipeline
# on purpose. Under Windows PowerShell 5.1, ConvertFrom-Json emits a JSON array
# as a single, non-enumerated pipeline object, so both
#   @(Get-Body | ConvertFrom-Json)          # -> 1-element array wrapping the array
#   Get-Body | ConvertFrom-Json | Where-...  # -> $_ is the whole array
# leak the entire array through the filter, and $asset.size then blows up as an
# Object[]. foreach over the assigned variable enumerates the array's elements
# identically on 5.1 and 7, and returns a single object.
function Find-Asset {
    param([string]$Name)
    $parsed = Get-Body | ConvertFrom-Json
    foreach ($asset in $parsed) {
        if ($asset.name -eq $Name) { return $asset }
    }
    return $null
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

    $code = Invoke-Api -Method 'POST' -Url "$api/repos/$repo/releases" `
        -ExtraArgs @('-H', 'Content-Type: application/json', '--data-binary', "@$jsonFile")
    switch ($code) {
        '201' { Write-Host "created release $tag" }
        '422' { Write-Host "release $tag already exists" }
        default { throw (Get-FailureMessage "failed to create release (HTTP $code)") }
    }

    # --- resolve the release id ----------------------------------------------

    $code = Invoke-Api -Method 'GET' -Url "$api/repos/$repo/releases/tags/$tag"
    if ($code -ne '200') { throw (Get-FailureMessage "failed to look up release $tag (HTTP $code)") }
    $releaseId = (Get-Body | ConvertFrom-Json).id
    if (-not $releaseId) { throw (Get-FailureMessage 'release lookup returned no id') }

    # --- publish the checksum file, then the binary -------------------------

    $digest = (Get-FileHash -Path $staged -Algorithm SHA256).Hash.ToLowerInvariant()
    $sumsName = "$artifact.sha256"
    $sumsFile = Join-Path 'artifacts' $sumsName
    Write-Utf8NoBom -Path $sumsFile -Content "$digest  $artifact`n"

    $assetsUrl = "$api/repos/$repo/releases/$releaseId/assets?per_page=100"

    function Find-ReleaseAsset {
        param([string]$Name)
        $code = Invoke-Api -Method 'GET' -Url $assetsUrl
        if ($code -ne '200') { throw (Get-FailureMessage "failed to list release assets (HTTP $code)") }
        return (Find-Asset -Name $Name)
    }

    function Publish-Asset {
        param([string]$Name, [string]$Path)
        $code = Invoke-Api -Method 'POST' -Url "$uploads/repos/$repo/releases/$releaseId/assets?name=$Name" `
            -ExtraArgs @('-H', 'Content-Type: application/octet-stream', '--data-binary', "@$Path")
        if ($code -ne '201') { throw (Get-FailureMessage "failed to upload $Name (HTTP $code)") }
        $expected = (Get-Item $Path).Length
        $asset = Find-ReleaseAsset -Name $Name
        if (-not $asset) { throw "$Name is missing from the release after a successful upload" }
        if ($asset.state -ne 'uploaded') { throw "$Name is in state '$($asset.state)', expected uploaded" }
        if ([long]$asset.size -ne [long]$expected) {
            throw "$Name is $($asset.size) bytes on the release, expected $expected"
        }
        Write-Host "verified $Name ($($asset.size) bytes)"
    }

    $publishedSums = Find-ReleaseAsset -Name $sumsName
    $publishedBin = Find-ReleaseAsset -Name $artifact

    if ($publishedSums) {
        # Assets of a published release are public; no token is sent.
        $text = & curl.exe -fsSL $publishedSums.browser_download_url
        if ($LASTEXITCODE -ne 0) { throw "could not read the published $sumsName" }
        $published = ("$text".Trim() -split '\s+')[0]
        if ($published -ne $digest) {
            throw "release $tag already publishes a different $artifact (sha256 $published); a published binary is never replaced, tag a new version"
        }
        Write-Host "$sumsName already published and matches"
    }
    elseif ($publishedBin) {
        throw "release $tag already has $artifact without a checksum file; a published binary is never replaced, tag a new version"
    }
    else {
        Publish-Asset -Name $sumsName -Path $sumsFile
    }

    if ($publishedBin) {
        Write-Host "$artifact already published and matches its checksum"
    }
    else {
        Publish-Asset -Name $artifact -Path $staged
    }
}
catch {
    Write-Host "release upload failed: $_" -ForegroundColor Red
    exit 1
}
finally {
    Remove-Item $bodyFile, $jsonFile -Force -ErrorAction SilentlyContinue
}

exit 0
