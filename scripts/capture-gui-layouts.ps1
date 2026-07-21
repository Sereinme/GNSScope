param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\artifacts\gui-layouts"),
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class GnsscopeWindowCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr window, IntPtr deviceContext, uint flags);

    [DllImport("user32.dll")]
    public static extern uint GetDpiForWindow(IntPtr window);
}
"@
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$outputPath = [System.IO.Path]::GetFullPath($OutputDirectory)
$binaryPath = Join-Path $repositoryRoot "target\release\gnss-gui.exe"

if (-not $SkipBuild) {
    & rtk cargo build --release -p gnss-gui
    if ($LASTEXITCODE -ne 0) {
        throw "Release GUI build failed"
    }
}
if (-not (Test-Path -LiteralPath $binaryPath)) {
    throw "GUI binary not found at $binaryPath"
}

New-Item -ItemType Directory -Force -Path $outputPath | Out-Null
$layouts = @(
    @{ Name = "desktop"; Width = 1440; Height = 900; Theme = "dark"; SettingsCategory = "input"; OpenSettings = $false; OpenComparison = $false },
    @{ Name = "compact-drawer"; Width = 1100; Height = 800; Theme = "dark"; SettingsCategory = "input"; OpenSettings = $true; OpenComparison = $false },
    @{ Name = "settings-tracking"; Width = 1440; Height = 900; Theme = "dark"; SettingsCategory = "tracking"; OpenSettings = $true; OpenComparison = $false },
    @{ Name = "settings-light"; Width = 1440; Height = 900; Theme = "light"; SettingsCategory = "input"; OpenSettings = $true; OpenComparison = $false },
    @{ Name = "candidate-comparison"; Width = 1440; Height = 900; Theme = "dark"; SettingsCategory = "input"; OpenSettings = $false; OpenComparison = $true },
    @{ Name = "candidate-comparison-compact"; Width = 1100; Height = 800; Theme = "dark"; SettingsCategory = "input"; OpenSettings = $false; OpenComparison = $true },
    @{ Name = "narrow"; Width = 800; Height = 720; Theme = "dark"; SettingsCategory = "input"; OpenSettings = $false; OpenComparison = $false }
)

foreach ($layout in $layouts) {
    $processInfo = [System.Diagnostics.ProcessStartInfo]::new($binaryPath)
    $processInfo.WorkingDirectory = $repositoryRoot
    $processInfo.UseShellExecute = $false
    $processInfo.Environment["GNSSCOPE_WINDOW_WIDTH"] = $layout.Width.ToString()
    $processInfo.Environment["GNSSCOPE_WINDOW_HEIGHT"] = $layout.Height.ToString()
    $processInfo.Environment["GNSSCOPE_PREVIEW"] = "1"
    $processInfo.Environment["GNSSCOPE_THEME"] = $layout.Theme
    $processInfo.Environment["GNSSCOPE_SETTINGS_CATEGORY"] = $layout.SettingsCategory
    $processInfo.Environment["GNSSCOPE_OPEN_SETTINGS"] = if ($layout.OpenSettings) { "1" } else { "0" }
    $processInfo.Environment["GNSSCOPE_OPEN_COMPARISON"] = if ($layout.OpenComparison) { "1" } else { "0" }
    $processInfo.Environment["GNSSCOPE_START_DRAFT"] = if ($layout.OpenSettings) { "1" } else { "0" }
    $process = [System.Diagnostics.Process]::Start($processInfo)
    try {
        Start-Sleep -Seconds 3
        $process.Refresh()
        if ($process.MainWindowHandle -eq [IntPtr]::Zero) {
            throw "GUI window was not created for $($layout.Name) layout"
        }
        $rect = [GnsscopeWindowCapture+Rect]::new()
        if (-not [GnsscopeWindowCapture]::GetWindowRect($process.MainWindowHandle, [ref]$rect)) {
            throw "Unable to read window bounds for $($layout.Name) layout"
        }
        $dpiScale = [GnsscopeWindowCapture]::GetDpiForWindow($process.MainWindowHandle) / 96.0
        $width = [int][Math]::Ceiling(($rect.Right - $rect.Left) * $dpiScale)
        $height = [int][Math]::Ceiling(($rect.Bottom - $rect.Top) * $dpiScale)
        $bitmap = [System.Drawing.Bitmap]::new($width, $height)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $deviceContext = $graphics.GetHdc()
        try {
            if (-not [GnsscopeWindowCapture]::PrintWindow($process.MainWindowHandle, $deviceContext, 2)) {
                throw "Unable to capture $($layout.Name) layout"
            }
        }
        finally {
            $graphics.ReleaseHdc($deviceContext)
            $graphics.Dispose()
        }
        $destination = Join-Path $outputPath "$($layout.Name)-$($layout.Width)x$($layout.Height).png"
        $bitmap.Save($destination, [System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Dispose()
        if ((Get-Item -LiteralPath $destination).Length -le 128) {
            throw "Screenshot is empty: $destination"
        }
        Write-Host "Captured $destination"
    }
    finally {
        if (-not $process.HasExited) {
            $process.Kill()
            $process.WaitForExit()
        }
    }
}
