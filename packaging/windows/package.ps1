param(
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string]$Work,
    [Parameter(Mandatory)][string]$Output
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Binary -PathType Leaf)) { throw "Missing executable: $Binary" }
$version = $env:S3_STUDIO_VERSION
$source = $PSScriptRoot
$icon = (Resolve-Path 'src-nsh/icons/icon.ico').Path
$wix = if ($env:WIX_BIN) { $env:WIX_BIN } elseif ($env:WIX) {
    Join-Path $env:WIX 'bin'
} else {
    Split-Path (Get-Command candle.exe).Source
}
$nsis = Get-Command makensis.exe -ErrorAction SilentlyContinue
$makensis = if ($nsis) { $nsis.Source } else { "${env:ProgramFiles(x86)}\NSIS\makensis.exe" }
$msiMarker = Join-Path $Work 's3-studio.install'
Set-Content -LiteralPath $msiMarker -Value 'msi' -Encoding ascii -NoNewline

& $makensis "/DAPP_VERSION=$version" "/DAPP_BINARY=$Binary" "/DAPP_ICON=$icon" `
    "/DOUTPUT_FILE=$Output\S3-Studio_${version}_windows_x64-setup.exe" (Join-Path $source 'installer.nsi')
if ($LASTEXITCODE -ne 0) { throw "NSIS failed: $LASTEXITCODE" }

$object = Join-Path $Work 'installer.wixobj'
& (Join-Path $wix 'candle.exe') -nologo -arch x64 "-dAppVersion=$version" "-dAppBinary=$Binary" `
    "-dAppIcon=$icon" "-dInstallMarker=$msiMarker" -out $object (Join-Path $source 'installer.wxs')
if ($LASTEXITCODE -ne 0) { throw "WiX compilation failed: $LASTEXITCODE" }
& (Join-Path $wix 'light.exe') -nologo -ext WixUIExtension -out "$Output\S3-Studio_${version}_windows_x64.msi" $object
if ($LASTEXITCODE -ne 0) { throw "WiX linking failed: $LASTEXITCODE" }
