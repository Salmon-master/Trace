[CmdletBinding()]
param(
    [ValidateSet("auto", "codex", "claude", "cursor", "custom")]
    [string]$Agent = "auto",
    [ValidateSet("global", "project")]
    [string]$Scope = "global",
    [string]$SkillDirectory,
    [string]$BundleRoot,
    [string]$BinaryDirectory,
    [switch]$Force,
    [switch]$ListAgents,
    [switch]$NoPath
)

$ErrorActionPreference = "Stop"

function Test-CommandAvailable {
    param([string]$Name)
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Get-AgentSpecs {
    return [ordered]@{
        codex = [pscustomobject]@{
            Name = "Codex"
            Commands = @("codex")
            GlobalRoot = Join-Path $env:USERPROFILE ".codex\skills"
            ProjectRoot = ".codex\skills"
        }
        claude = [pscustomobject]@{
            Name = "Claude Code"
            Commands = @("claude")
            GlobalRoot = Join-Path $env:USERPROFILE ".claude\skills"
            ProjectRoot = ".claude\skills"
        }
        cursor = [pscustomobject]@{
            Name = "Cursor"
            Commands = @("cursor")
            GlobalRoot = Join-Path $env:USERPROFILE ".cursor\skills"
            ProjectRoot = ".cursor\skills"
        }
    }
}

function Get-DetectedAgents {
    param($Specs)
    $detected = @()
    foreach ($entry in $Specs.GetEnumerator()) {
        $directoryExists = Test-Path -LiteralPath $entry.Value.GlobalRoot
        $commandExists = $false
        foreach ($command in $entry.Value.Commands) {
            if (Test-CommandAvailable $command) {
                $commandExists = $true
            }
        }
        if ($directoryExists -or $commandExists) {
            $detected += $entry.Key
        }
    }
    return $detected
}

function Select-Agent {
    param($Specs, [string[]]$Detected)

    if ($Detected.Count -eq 0) {
        throw "No supported agent was detected. Use -Agent codex, -Agent claude, -Agent cursor, or -Agent custom -SkillDirectory <path>."
    }
    if ($Detected.Count -eq 1) {
        return $Detected[0]
    }

    Write-Host "Detected agent tools:"
    for ($index = 0; $index -lt $Detected.Count; $index++) {
        $key = $Detected[$index]
        Write-Host ("  {0}. {1}" -f ($index + 1), $Specs[$key].Name)
    }
    $selection = Read-Host "Select the agent to install Trace into"
    $number = 0
    if (-not [int]::TryParse($selection, [ref]$number) -or $number -lt 1 -or $number -gt $Detected.Count) {
        throw "Invalid agent selection."
    }
    return $Detected[$number - 1]
}

function Get-BundleRootPath {
    if ($BundleRoot) {
        return (Resolve-Path -LiteralPath $BundleRoot).Path
    }
    return (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

function Add-UserPathEntry {
    param([string]$Directory)
    $current = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @($current -split ";" | Where-Object { $_ -and $_.Trim() })
    if (-not ($entries | Where-Object { $_.TrimEnd("\") -ieq $Directory.TrimEnd("\") })) {
        $updated = (($entries + $Directory) -join ";")
        [Environment]::SetEnvironmentVariable("Path", $updated, "User")
    }
    if (-not (($env:Path -split ";") | Where-Object { $_.TrimEnd("\") -ieq $Directory.TrimEnd("\") })) {
        $env:Path = "$Directory;$env:Path"
    }
}

function Backup-ExistingDirectory {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }
    if (-not $Force) {
        throw "$Path already exists. Re-run with -Force to replace it."
    }
    $backup = "$Path.backup-$(Get-Date -Format yyyyMMddHHmmss)"
    Move-Item -LiteralPath $Path -Destination $backup
    Write-Host "Existing skill backed up to $backup"
}

$specs = Get-AgentSpecs
if ($ListAgents) {
    foreach ($entry in $specs.GetEnumerator()) {
        $status = if ((Get-DetectedAgents $specs) -contains $entry.Key) { "detected" } else { "not detected" }
        Write-Output ("{0}: {1}" -f $entry.Value.Name, $status)
    }
    exit 0
}

$root = Get-BundleRootPath
$binarySource = Join-Path $root "trace.exe"
$skillSource = Join-Path $root "skill\trace-hardware"
if (-not (Test-Path -LiteralPath $binarySource)) {
    throw "trace.exe was not found in $root. Run this installer from a Trace release bundle."
}
if (-not (Test-Path -LiteralPath (Join-Path $skillSource "SKILL.md"))) {
    throw "skill\trace-hardware\SKILL.md was not found in $root."
}

$detected = @(Get-DetectedAgents $specs)
$selected = if ($Agent -eq "auto") { Select-Agent $specs $detected } else { $Agent }

$skillTarget = if ($selected -eq "custom") {
    if (-not $SkillDirectory) {
        throw "-SkillDirectory is required with -Agent custom."
    }
    Join-Path $SkillDirectory "trace-hardware"
} elseif ($Scope -eq "project") {
    Join-Path (Join-Path (Get-Location).Path $specs[$selected].ProjectRoot) "trace-hardware"
} else {
    Join-Path $specs[$selected].GlobalRoot "trace-hardware"
}

$binaryTargetRoot = if ($BinaryDirectory) {
    [System.IO.Path]::GetFullPath($BinaryDirectory)
} else {
    Join-Path $env:LOCALAPPDATA "Trace\bin"
}
$binaryTarget = Join-Path $binaryTargetRoot "trace.exe"
New-Item -ItemType Directory -Path $binaryTargetRoot -Force | Out-Null
New-Item -ItemType Directory -Path (Split-Path -Parent $skillTarget) -Force | Out-Null
Backup-ExistingDirectory $skillTarget
Copy-Item -LiteralPath $binarySource -Destination $binaryTarget -Force
Copy-Item -LiteralPath $skillSource -Destination $skillTarget -Recurse

if (-not $NoPath) {
    Add-UserPathEntry $binaryTargetRoot
}

Write-Host "Installed Trace CLI to $binaryTarget"
$selectedName = if ($selected -eq "custom") { "custom agent" } else { $specs[$selected].Name }
Write-Host "Installed trace-hardware skill for $selectedName to $skillTarget"
if (Test-CommandAvailable "kicad-cli") {
    Write-Host "KiCad CLI detected."
} else {
    Write-Host "KiCad CLI not detected; install KiCad or set TRACE_KICAD_CLI before using schematic commands."
}
Write-Host "Open a new terminal, then run: trace --version"
