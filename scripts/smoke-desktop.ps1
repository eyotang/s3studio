param([string]$Executable = 'src-tauri/target/release/s3-studio.exe')
$ErrorActionPreference = 'Stop'
$projectDirectory = Split-Path -Parent $PSScriptRoot
$executablePath = (Resolve-Path -LiteralPath (Join-Path $projectDirectory $Executable)).Path
$artifactDirectory = Join-Path $projectDirectory 'artifacts'
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
$stdoutPath = Join-Path $artifactDirectory 'native-startup-stdout.log'
$stderrPath = Join-Path $artifactDirectory 'native-startup-stderr.log'
$nativeProcess = Start-Process -FilePath $executablePath -PassThru -WindowStyle Normal -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
try {
    $ready = $nativeProcess.WaitForInputIdle(15000)
    # Window creation can precede renderer initialization on a fresh CI machine.
    # Require the real window to remain responsive, not merely acquire its title.
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    $responsiveSamples = 0
    do {
        $nativeProcess.Refresh()
        if ($nativeProcess.HasExited) { break }
        if ($nativeProcess.MainWindowTitle -eq 'S3 Studio' -and $nativeProcess.Responding) {
            $responsiveSamples++
        } else {
            $responsiveSamples = 0
        }
        if ($responsiveSamples -ge 3) { break }
        Start-Sleep -Milliseconds 1000
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($nativeProcess.HasExited) {
        throw "Desktop exited with code $($nativeProcess.ExitCode): $(Get-Content -LiteralPath $stderrPath -Raw)"
    }
    $report = [ordered]@{
        executable = $executablePath
        alive = -not $nativeProcess.HasExited
        inputIdle = $ready
        responding = $nativeProcess.Responding
        title = $nativeProcess.MainWindowTitle
        responsiveSamples = $responsiveSamples
        stderr = (Get-Content -LiteralPath $stderrPath -Raw)
    }
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $artifactDirectory 'native-startup.json') -Encoding utf8
    if ($responsiveSamples -lt 3) { throw "Desktop did not open a responsive S3 Studio window: $($report | ConvertTo-Json -Compress)" }
    $report | ConvertTo-Json
} finally {
    $nativeProcess.Refresh()
    if (-not $nativeProcess.HasExited) { Stop-Process -Id $nativeProcess.Id }
}
