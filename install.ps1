# Project Telescope installer for Windows
# Usage: irm https://raw.githubusercontent.com/microsoft/project-telescope/main/install.ps1 | iex

$ErrorActionPreference = 'Stop'

$Repo = "microsoft/project-telescope"
$Version = if ($env:TELESCOPE_VERSION) { $env:TELESCOPE_VERSION } else { "latest" }

function Get-Arch {
    switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { return "x64" }
        "ARM64" { return "arm64" }
        default { throw "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
    }
}

function Get-DownloadUrl {
    param([string]$Arch)
    $AssetName = "telescope-msi-${Arch}.zip"
    if ($Version -eq "latest") {
        return "https://github.com/$Repo/releases/latest/download/$AssetName"
    } else {
        return "https://github.com/$Repo/releases/download/$Version/$AssetName"
    }
}

function Install-Telescope {
    $Arch = Get-Arch
    $Url = Get-DownloadUrl -Arch $Arch

    Write-Host "Installing Project Telescope for windows/$Arch..." -ForegroundColor Cyan
    Write-Host "Download: $Url" -ForegroundColor Cyan

    $TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

    try {
        $ZipPath = Join-Path $TmpDir "telescope-msi.zip"
        $ExtractPath = Join-Path $TmpDir "extracted"

        # Download
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $Url -OutFile $ZipPath -UseBasicParsing

        # Extract the MSI from the zip
        Expand-Archive -Path $ZipPath -DestinationPath $ExtractPath -Force

        # Find the MSI file
        $Msi = Get-ChildItem -Path $ExtractPath -Filter "*.msi" -Recurse | Select-Object -First 1
        if (-not $Msi) {
            throw "No MSI file found in the downloaded archive"
        }

        Write-Host "Running installer: $($Msi.Name)" -ForegroundColor Cyan

        # Run the MSI installer
        $MsiArgs = "/i `"$($Msi.FullName)`" /quiet /norestart"
        $Process = Start-Process -FilePath "msiexec.exe" -ArgumentList $MsiArgs -Wait -PassThru

        if ($Process.ExitCode -eq 0) {
            Write-Host ""
            Write-Host "Project Telescope is installed! Run 'tele --help' to get started." -ForegroundColor Green
            Write-Host "You may need to restart your terminal for PATH changes to take effect." -ForegroundColor Yellow
        } elseif ($Process.ExitCode -eq 3010) {
            Write-Host ""
            Write-Host "Project Telescope is installed! A restart is required to complete setup." -ForegroundColor Yellow
        } else {
            throw "MSI installer failed with exit code $($Process.ExitCode)"
        }
    }
    finally {
        Remove-Item -Recurse -Force -Path $TmpDir -ErrorAction SilentlyContinue
    }
}

Install-Telescope
