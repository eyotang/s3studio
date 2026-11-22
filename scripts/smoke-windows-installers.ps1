$ErrorActionPreference = 'Stop'
$previous = '0.4.7'
$version = (Get-Content packaging/app.json -Raw | ConvertFrom-Json).version
$work = Join-Path $env:RUNNER_TEMP 's3-studio-installer-smoke'
$legacy = Join-Path $work 'previous'
$bundle = (Resolve-Path 'src-nsh/target/installer/bundle').Path
$binary = (Resolve-Path 'src-nsh/target/installer/release/s3-studio.exe').Path
$expectedHash = (Get-FileHash $binary).Hash
New-Item -ItemType Directory -Path $legacy -Force | Out-Null
$patterns = @('--pattern', '*_windows_x64-setup.exe', '--pattern', '*_windows_x64.msi', '--pattern', 'SHA256SUMS.txt')
gh release download "v$previous" --repo eyotang/s3studio --dir $legacy @patterns
if ($LASTEXITCODE -ne 0) { throw 'Could not download the previous installers' }
$checksums = Get-Content (Join-Path $legacy 'SHA256SUMS.txt')

function Previous-Installer([string]$Suffix) {
    $name = "S3-Studio_${previous}_windows_x64$Suffix"
    $path = Join-Path $legacy $name
    $expected = "$((Get-FileHash $path).Hash.ToLowerInvariant())  $name"
    if ($checksums -cnotcontains $expected) { throw "Checksum mismatch: $name" }
    return $path
}

function Run-Installer([string]$Program, [string]$Arguments) {
    $process = Start-Process $Program -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "Installer failed with exit code $($process.ExitCode): $Program $Arguments"
    }
}

function Verify-Installed([string]$Directory, [string]$Kind) {
    $installed = Join-Path $Directory 's3-studio.exe'
    if ((Get-FileHash $installed).Hash -ne $expectedHash) {
        throw "$Kind did not install the Native Shell executable"
    }
    $marker = Get-Content (Join-Path $Directory 's3-studio.install') -Raw
    if ($marker -cne $Kind) { throw "Incorrect update package marker: $marker" }
}

$nsisBefore = Previous-Installer '-setup.exe'
$nsisAfter = Join-Path $bundle "S3-Studio_${version}_windows_x64-setup.exe"
$nsisDir = Join-Path $work 'NSIS custom directory'
Run-Installer $nsisBefore "/S /D=$nsisDir"
if (-not (Test-Path (Join-Path $nsisDir 's3-studio.exe'))) { throw 'Previous NSIS install failed' }
Run-Installer $nsisAfter '/P /UPDATE'
Verify-Installed $nsisDir 'nsis'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\S3 Studio'
if ((Get-ItemProperty $uninstallKey).DisplayVersion -ne $version) { throw 'NSIS upgrade registration failed' }
Run-Installer (Join-Path $nsisDir 'uninstall.exe') "/S _?=$nsisDir"
if (Test-Path $uninstallKey) { throw 'NSIS uninstall registration remains' }
Write-Host "NSIS upgraded the previous release in its custom directory and uninstalled."

$msiBefore = Previous-Installer '.msi'
$msiAfter = Join-Path $bundle "S3-Studio_${version}_windows_x64.msi"
$msiDir = Join-Path $work 'MSI custom directory'
$msiexec = Join-Path $env:SystemRoot 'System32\msiexec.exe'
Run-Installer $msiexec ('/i "{0}" /quiet /norestart INSTALLDIR="{1}" /l*v "{2}\previous-msi.log"' -f $msiBefore, $msiDir, $work)
if (-not (Test-Path (Join-Path $msiDir 's3-studio.exe'))) { throw 'Previous MSI install failed' }
Run-Installer $msiexec ('/i "{0}" /quiet /norestart /l*v "{1}\upgrade-msi.log"' -f $msiAfter, $work)
Verify-Installed $msiDir 'msi'
$msi = @(Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' |
    Where-Object DisplayName -eq 'S3 Studio')
if ($msi.Count -ne 1 -or $msi[0].DisplayVersion -ne $version) {
    throw 'MSI upgrade registration did not replace the previous version'
}
Run-Installer $msiexec ('/x "{0}" /quiet /norestart' -f $msiAfter)
if (Test-Path (Join-Path $msiDir 's3-studio.exe')) { throw 'MSI executable remains after uninstall' }
Write-Host "MSI upgraded the previous release in its custom directory and uninstalled."
