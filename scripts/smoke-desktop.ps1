param([string]$Executable = 'src-tauri/target/release/s3-studio.exe')
$ErrorActionPreference = 'Stop'
$projectDirectory = Split-Path -Parent $PSScriptRoot
$executablePath = (Resolve-Path -LiteralPath (Join-Path $projectDirectory $Executable)).Path
$artifactDirectory = Join-Path $projectDirectory 'artifacts'
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
$stdoutPath = Join-Path $artifactDirectory 'native-startup-stdout.log'
$stderrPath = Join-Path $artifactDirectory 'native-startup-stderr.log'
$nativeProcess = Start-Process -FilePath $executablePath -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
try {
    $ready = $nativeProcess.WaitForInputIdle(15000)
    Start-Sleep -Seconds 3
    $nativeProcess.Refresh()
    if ($nativeProcess.HasExited) {
        throw "Desktop exited with code $($nativeProcess.ExitCode): $(Get-Content -LiteralPath $stderrPath -Raw)"
    }
    $report = [ordered]@{
        executable = $executablePath
        alive = -not $nativeProcess.HasExited
        inputIdle = $ready
        responding = $nativeProcess.Responding
        title = $nativeProcess.MainWindowTitle
        stderr = (Get-Content -LiteralPath $stderrPath -Raw)
    }
    if (-not $ready -or -not $nativeProcess.Responding) { throw 'Desktop did not become responsive.' }
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $artifactDirectory 'native-startup.json') -Encoding utf8
    $report | ConvertTo-Json
} finally {
    $nativeProcess.Refresh()
    if (-not $nativeProcess.HasExited) { Stop-Process -Id $nativeProcess.Id }
}
