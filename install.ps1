# Project Telescope installer for Windows
# Usage: irm https://raw.githubusercontent.com/microsoft/project-telescope/main/install.ps1 | iex

$ErrorActionPreference = 'Stop'

$Repo = "microsoft/project-telescope"
$Version = if ($env:TELESCOPE_VERSION) { $env:TELESCOPE_VERSION } else { "latest" }
$InstallDir = if ($env:TELESCOPE_INSTALL_DIR) { $env:TELESCOPE_INSTALL_DIR } else { Join-Path $env:USERPROFILE ".telescope" }
$BinDir = Join-Path $InstallDir "bin"

function Get-Arch {
    <#
    .SYNOPSIS
    Returns the release architecture for the current process.
    #>
    switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { return "x64" }
        "ARM64" { return "arm64" }
        default { throw "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
    }
}

function Get-Release {
    <#
    .SYNOPSIS
    Returns GitHub release metadata for the requested Telescope version.
    #>
    if ($Version -eq "latest") {
        return Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
    }

    $Tag = [Uri]::EscapeDataString($Version)
    return Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$Tag" -UseBasicParsing
}

function Get-Asset {
    <#
    .SYNOPSIS
    Returns a named asset from GitHub release metadata.
    #>
    param(
        [Parameter(Mandatory)]
        [object]$Release,

        [Parameter(Mandatory)]
        [string]$Name
    )

    $Asset = $Release.assets |
        Where-Object -Property name -EQ $Name |
        Select-Object -First 1

    if (-not $Asset) {
        throw "Release asset not found: $Name"
    }

    return $Asset
}

function Assert-AssetHash {
    <#
    .SYNOPSIS
    Verifies a downloaded release asset against its GitHub SHA-256 digest.
    #>
    param(
        [Parameter(Mandatory)]
        [string]$Path,

        [Parameter(Mandatory)]
        [object]$Asset
    )

    $Sha256Pattern = '^sha256:([0-9a-fA-F]{64})$'
    if (-not $Asset.digest -or $Asset.digest -notmatch $Sha256Pattern) {
        throw "Release asset '$($Asset.name)' does not include a SHA-256 digest."
    }

    $ExpectedHash = $Matches[1]
    $ActualHash = (Get-FileHash -Path $Path -Algorithm SHA256).Hash
    if ($ActualHash -ne $ExpectedHash) {
        throw "SHA-256 mismatch for release asset '$($Asset.name)'."
    }
}

function Save-Asset {
    <#
    .SYNOPSIS
    Downloads and verifies a GitHub release asset.
    #>
    param(
        [Parameter(Mandatory)]
        [object]$Asset,

        [Parameter(Mandatory)]
        [string]$Path
    )

    Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $Path -UseBasicParsing
    Assert-AssetHash -Path $Path -Asset $Asset
}

function Install-Telescope {
    <#
    .SYNOPSIS
    Installs the verified Telescope release for Windows.
    #>
    $Arch = Get-Arch
    $Release = Get-Release
    $Tag = $Release.tag_name
    $Ver = $Tag -replace '^v', ''

    # Prefer MSI installer on Windows
    $MsiAssetName = "telescope-${Arch}.msi"
    $MsiAsset = $Release.assets |
        Where-Object -Property name -EQ $MsiAssetName |
        Select-Object -First 1

    Write-Host "Installing Project Telescope for windows/$Arch..." -ForegroundColor Cyan

    $TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

        if ($MsiAsset) {
            $MsiPath = Join-Path $TmpDir $MsiAssetName
            Save-Asset -Asset $MsiAsset -Path $MsiPath

            Write-Host "Installing via MSI: $MsiAssetName" -ForegroundColor Cyan
            Start-Process msiexec.exe -ArgumentList "/i `"$MsiPath`" /quiet /norestart" -Wait -NoNewWindow
            Write-Host ""
            Write-Host "Project Telescope installed via MSI." -ForegroundColor Green
        } else {
            # Fall back to zip archive
            $ZipAssetName = "telescope-${Ver}-windows-${Arch}.zip"
            $ZipAsset = Get-Asset -Release $Release -Name $ZipAssetName
            Write-Host "Download: $($ZipAsset.browser_download_url)" -ForegroundColor Cyan

            $ZipPath = Join-Path $TmpDir $ZipAssetName
            $ExtractPath = Join-Path $TmpDir "extracted"

            Save-Asset -Asset $ZipAsset -Path $ZipPath
            Expand-Archive -Path $ZipPath -DestinationPath $ExtractPath -Force

            # Stop running instances so we can overwrite
            Get-Process -Name "tele","telescope-service" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 500

            # Install binaries
            New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
            Get-ChildItem -Path $ExtractPath -Recurse -File | Where-Object { $_.Extension -ne '.d' } | ForEach-Object {
                Copy-Item -Path $_.FullName -Destination $BinDir -Force
            }

            Write-Host ""
            Write-Host "Project Telescope installed to ${BinDir}" -ForegroundColor Green

            # Add to PATH for current user if not already present
            $UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
            if ($UserPath -notlike "*$BinDir*") {
                [Environment]::SetEnvironmentVariable("PATH", "$BinDir;$UserPath", "User")
                Write-Host "Added ${BinDir} to your user PATH." -ForegroundColor Cyan
                Write-Host "You may need to restart your terminal for PATH changes to take effect." -ForegroundColor Yellow
            }
        }

        # Verify
        $TeleExe = Join-Path $BinDir "tele.exe"
        if (Test-Path $TeleExe) {
            & $TeleExe --version 2>$null
            Write-Host ""
            Write-Host "Quick reference:" -ForegroundColor White
            Write-Host "  tele service start    - start service + collectors" -ForegroundColor Gray
            Write-Host "  tele service status   - check service status" -ForegroundColor Gray
        }
    }
    finally {
        Remove-Item -Recurse -Force -Path $TmpDir -ErrorAction SilentlyContinue
    }
}

Install-Telescope
