BeforeAll {
    $InstallScript = Join-Path $PSScriptRoot "..\install.ps1"
    $script:WingetWorkflow = Join-Path $PSScriptRoot "..\.github\workflows\winget.yml"

    $Tokens = $null
    $Errors = $null
    $Ast = [System.Management.Automation.Language.Parser]::ParseFile(
        $InstallScript,
        [ref]$Tokens,
        [ref]$Errors
    )

    $Functions = $Ast.FindAll({
        param($Node)

        $Node -is [System.Management.Automation.Language.FunctionDefinitionAst]
    }, $false)

    foreach ($Function in $Functions) {
        . ([scriptblock]::Create($Function.Extent.Text))
    }
}

Describe "Windows installer integrity" {
    It "accepts a matching SHA-256 digest" {
        $AssetPath = Join-Path $TestDrive "asset.zip"
        Set-Content -Path $AssetPath -Value "trusted release asset" -NoNewline
        $Digest = "sha256:$((Get-FileHash -Path $AssetPath -Algorithm SHA256).Hash.ToLowerInvariant())"
        $Asset = [pscustomobject]@{ name = "asset.zip"; digest = $Digest }

        { Assert-AssetHash -Path $AssetPath -Asset $Asset } | Should -Not -Throw
    }

    It "rejects a mismatched SHA-256 digest" {
        $AssetPath = Join-Path $TestDrive "asset.zip"
        Set-Content -Path $AssetPath -Value "tampered release asset" -NoNewline
        $Asset = [pscustomobject]@{ name = "asset.zip"; digest = "sha256:$("0" * 64)" }

        { Assert-AssetHash -Path $AssetPath -Asset $Asset } |
            Should -Throw "*SHA-256 mismatch*"
    }

    It "rejects release assets without a digest" {
        $AssetPath = Join-Path $TestDrive "asset.zip"
        Set-Content -Path $AssetPath -Value "unverifiable release asset" -NoNewline
        $Asset = [pscustomobject]@{ name = "asset.zip"; digest = $null }

        { Assert-AssetHash -Path $AssetPath -Asset $Asset } |
            Should -Throw "*does not include a SHA-256 digest*"
    }

    It "verifies every release asset download" {
        $Install = Get-Content -Path $InstallScript -Raw

        ([regex]::Matches($Install, "Invoke-WebRequest")).Count | Should -Be 1
        $Install | Should -Match "(?s)function Save-Asset.*Invoke-WebRequest.*Assert-AssetHash"
        $Install | Should -Match 'Save-Asset -Asset \$MsiAsset'
        $Install | Should -Match 'Save-Asset -Asset \$ZipAsset'
    }
}

Describe "WinGet workflow credentials" {
    It "does not reference a GitHub Actions secret" {
        $Workflow = Get-Content -Path $WingetWorkflow -Raw

        $Workflow | Should -Not -Match "secrets\."
        $Workflow | Should -Not -Match "WINGET_TOKEN"
    }
}
