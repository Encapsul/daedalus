# daedalus installer for Windows PowerShell — from GitHub Releases, no admin.
#
#   powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/Encapsul/daedalus/main/install.ps1 | iex"
#
# Installs daedalus.exe + daedalus-stub.exe into $env:LOCALAPPDATA\daedalus\bin.
# Overridable: $env:DAEDALUS_VERSION, $env:DAEDALUS_INSTALL_DIR,
#             $env:DAEDALUS_MIRROR (base URL, for mirror testing).

$ErrorActionPreference = "Stop"

function Get-LatestVersion {
    $resp = Invoke-RestMethod -Uri "https://api.github.com/repos/Encapsul/daedalus/releases/latest"
    return $resp.tag_name
}

$mirror   = if ($env:DAEDALUS_MIRROR) { $env:DAEDALUS_MIRROR } else { "https://github.com/Encapsul/daedalus/releases" }
$version  = if ($env:DAEDALUS_VERSION) { $env:DAEDALUS_VERSION } else { Get-LatestVersion }
if (-not $version) { $version = "v0.7.0" }
$installdir = if ($env:DAEDALUS_INSTALL_DIR) { $env:DAEDALUS_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "daedalus\bin" }

$asset = "daedalus_$($version.TrimStart('v'))_windows_amd64.tar.gz"
$rel   = "$mirror/download/$version"
Write-Host "daedalus installer: $asset from $version"

$tmp = Join-Path $env:TEMP ("daedalus-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    $archive = Join-Path $tmp $asset
    Write-Host "daedalus installer: downloading $asset ..."
    Invoke-WebRequest -Uri "$rel/$asset" -OutFile $archive -UseBasicParsing

    Write-Host "daedalus installer: verifying sha256 ..."
    $manifest = (Invoke-WebRequest -Uri "$rel/checksums.txt" -UseBasicParsing).Content
    $expected = ($manifest -split "`n" | ForEach-Object {
        $parts = $_ -split "\s+"
        if ($parts.Count -ge 2 -and $parts[1] -eq $asset) { $parts[0] }
    } | Select-Object -First 1)
    if (-not $expected) { throw "$asset missing from release checksums.txt" }
    $actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLower()
    if ($actual -ne $expected) { throw "sha256 mismatch: expected $expected got $actual" }

    tar -xf $archive -C $tmp
    New-Item -ItemType Directory -Force -Path $installdir | Out-Null
    Copy-Item -Force (Join-Path $tmp "daedalus_$($version.TrimStart('v'))_windows_amd64\daedalus.exe") $installdir
    Copy-Item -Force (Join-Path $tmp "daedalus_$($version.TrimStart('v'))_windows_amd64\daedalus-stub.exe") $installdir
    if (Test-Path (Join-Path $tmp "daedalus_$($version.TrimStart('v'))_windows_amd64\daedalus-crypto.exe")) {
        Copy-Item -Force (Join-Path $tmp "daedalus_$($version.TrimStart('v'))_windows_amd64\daedalus-crypto.exe") $installdir -ErrorAction SilentlyContinue
    }

    Write-Host ""
    Write-Host "Installed daedalus $version -> $installdir"
    Write-Host "Add it to your PATH:"
    Write-Host "  setx PATH \"$installdir;%PATH%\"   # new terminals only"
    Write-Host "Then open a new terminal and:"
    Write-Host "  daedalus build .\examples\offline-health-agri -o clinic-agent.de"
    Write-Host "  .\clinic-agent.de diagnose"
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}