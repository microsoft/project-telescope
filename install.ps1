# Project Telescope installer for Windows
# Usage: irm https://raw.githubusercontent.com/microsoft/project-telescope/main/install.ps1 | iex

$ErrorActionPreference = 'Stop'

$Repo = "microsoft/project-telescope"
$Version = if ($env:TELESCOPE_VERSION) { $env:TELESCOPE_VERSION } else { "latest" }
$InstallDir = if ($env:TELESCOPE_INSTALL_DIR) { $env:TELESCOPE_INSTALL_DIR } else { Join-Path $env:USERPROFILE ".telescope" }
$BinDir = Join-Path $InstallDir "bin"

function Get-Arch {
    switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { return "x64" }
        "ARM64" { return "arm64" }
        default { throw "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
    }
}

function Get-ReleaseTag {
    if ($Version -ne "latest") { return $Version }
    $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
    return $Release.tag_name
}

function Install-Telescope {
    $Arch = Get-Arch
    $Tag = Get-ReleaseTag
    $Ver = $Tag -replace '^v', ''

    # Prefer MSI installer on Windows
    $MsiAsset = "telescope-${Arch}.msi"
    $MsiUrl = "https://github.com/$Repo/releases/download/$Tag/$MsiAsset"

    Write-Host "Installing Project Telescope for windows/$Arch..." -ForegroundColor Cyan

    $TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

        # Try MSI first
        $MsiPath = Join-Path $TmpDir $MsiAsset
        $UseMsi = $true
        try {
            Invoke-WebRequest -Uri $MsiUrl -OutFile $MsiPath -UseBasicParsing
        } catch {
            $UseMsi = $false
        }

        if ($UseMsi) {
            Write-Host "Installing via MSI: $MsiAsset" -ForegroundColor Cyan
            Start-Process msiexec.exe -ArgumentList "/i `"$MsiPath`" /quiet /norestart" -Wait -NoNewWindow
            Write-Host ""
            Write-Host "Project Telescope installed via MSI." -ForegroundColor Green
        } else {
            # Fall back to zip archive
            $ZipAsset = "telescope-${Ver}-windows-${Arch}.zip"
            $ZipUrl = "https://github.com/$Repo/releases/download/$Tag/$ZipAsset"
            Write-Host "Download: $ZipUrl" -ForegroundColor Cyan

            $ZipPath = Join-Path $TmpDir $ZipAsset
            $ExtractPath = Join-Path $TmpDir "extracted"

            Invoke-WebRequest -Uri $ZipUrl -OutFile $ZipPath -UseBasicParsing
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
