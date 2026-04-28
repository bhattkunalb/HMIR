# scripts/install.ps1
# One-click HMIR installer for Windows 10/11 (PowerShell)
# Usage: irm https://raw.githubusercontent.com/bhattkunalb/HMIR/main/scripts/install.ps1 | iex
# Note: Run PowerShell as Administrator for NPU driver access (optional)

param(
    [switch]$DryRun,
    [switch]$SkipNPUCheck,
    [switch]$Local,
    [string]$InstallPath = "$env:USERPROFILE\.hmir"
)

# Set execution policy for the current session to avoid SecurityError on sub-scripts
Set-ExecutionPolicy Bypass -Scope Process -Force

# ========================================
# Configuration & Constants
# ========================================
$REPO = "bhattkunalb/HMIR"
$BUILD_TIMESTAMP = "2026-04-28-17:15" # Cache buster
$RELEASE_ENDPOINT = "https://api.github.com/repos/$REPO/releases/latest"
$API_PORT = 8080
$MIN_WINDOWS_BUILD = 19041  # Windows 10 20H2
$REQUIRED_NET_VERSION = "6.0"  # .NET 6+ for some dependencies

# ========================================
# Maintenance & Purge
# ========================================
function Invoke-ForcePurge {
    Write-Info " HMIR ELITE | PURGING STALE ENVIRONMENT"

    # 1. Force kill all related processes
    $targets = @("hmir", "hmir-api", "hmir-dashboard", "hmir-npu-worker", "hmir-e2e", "python", "uvicorn")
    foreach ($t in $targets) {
        $procs = Get-Process -Name $t -ErrorAction SilentlyContinue
        if ($procs) {
            Write-Warn "Force killing active process: $t"
            $procs | Stop-Process -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 300 # Give OS time to release file handles
        }
    }

    # 2. Robust Directory Purge (Rename-to-Delete strategy)
    if (Test-Path $InstallPath) {
        $timestamp = (Get-Date).Ticks
        $oldPath = "$InstallPath.old.$timestamp"
        
        Write-Info "Executing robust purge for $InstallPath..."
        
        # Strategy A: Rename the entire directory (Nearly instantaneous even with some locks)
        try {
            Rename-Item -Path $InstallPath -NewName (Split-Path $oldPath -Leaf) -ErrorAction Stop
            Write-Success "Moved stale environment to $oldPath"
            
            # Deletion can be slow (e.g. .venv with 10k+ files). Perform in background.
            Write-Info "Cleanup task queued in background."
            Start-Job -ScriptBlock { 
                param($path) 
                Remove-Item -Path $path -Recurse -Force -ErrorAction SilentlyContinue 
            } -ArgumentList $oldPath | Out-Null
        } catch {
            Write-Warn "Full directory rename failed (locked). Performing surgical binary purge..."
            
            # Strategy B: Rename individual scripts and binaries
            if (Test-Path "$InstallPath\scripts") {
                $oldScripts = "$InstallPath\scripts.old.$timestamp"
                Rename-Item -Path "$InstallPath\scripts" -NewName (Split-Path $oldScripts -Leaf) -ErrorAction SilentlyContinue
            }

            if (Test-Path "$InstallPath\.venv") {
                $oldVenv = "$InstallPath\venv.old.$timestamp"
                Rename-Item -Path "$InstallPath\.venv" -NewName (Split-Path $oldVenv -Leaf) -ErrorAction SilentlyContinue
                Start-Job -ScriptBlock { param($p) Remove-Item -Path $p -Recurse -Force } -ArgumentList $oldVenv | Out-Null
            }

            $binaries = Get-ChildItem -Path $InstallPath -Filter "*.exe" -ErrorAction SilentlyContinue
            foreach ($bin in $binaries) {
                $oldBin = "$($bin.FullName).$timestamp.old"
                Rename-Item $bin.FullName (Split-Path $oldBin -Leaf) -ErrorAction SilentlyContinue
                # Note: We don't delete these immediately as they might still be closing
            }
            
            Write-Success "Surgical purge complete. Stale binaries isolated."
        }
    }
}

# Colors for output
$ColorInfo = "Cyan"
$ColorSuccess = "Green"
$ColorWarn = "Yellow"
$ColorError = "Red"

function Write-Info    { param($msg) Write-Host "[INFO] " -ForegroundColor $ColorInfo -NoNewline; Write-Host $msg }
function Write-Success { param($msg) Write-Host "[+] " -ForegroundColor $ColorSuccess -NoNewline; Write-Host $msg }
function Write-Warn    { param($msg) Write-Host "[!] " -ForegroundColor $ColorWarn -NoNewline; Write-Host $msg }
function Write-Error   { param($msg) Write-Host "[-] " -ForegroundColor $ColorError -NoNewline; Write-Host $msg; exit 1 }

# ========================================
# System Checks
# ========================================
function Test-SystemRequirements {
    Write-Info "Checking system requirements..."

    # Windows version
    $winVersion = [System.Environment]::OSVersion.Version.Build
    if ($winVersion -lt $MIN_WINDOWS_BUILD) {
        Write-Warn "Windows build $winVersion detected. Minimum required: $MIN_WINDOWS_BUILD (Windows 10 20H2+)"
        Write-Warn "Continuing anyway, but some features may not work."
    }

    # PowerShell version
    if ($PSVersionTable.PSVersion.Major -lt 5) {
        Write-Error "PowerShell 5.0+ required. Current: $($PSVersionTable.PSVersion)"
    }

    # .NET runtime check (for some dependencies)
    try {
        $dotnetVersion = dotnet --version 2>$null
        if ($dotnetVersion -and [Version]$dotnetVersion.Split('.')[0] -lt [Version]$REQUIRED_NET_VERSION.Split('.')[0]) {
            Write-Warn ".NET $REQUIRED_NET_VERSION+ recommended for full compatibility. Found: $dotnetVersion"
        }
    } catch {
        Write-Warn "dotnet CLI not found. Some optional features may be unavailable."
    }

    # Administrator check for NPU drivers (optional)
    if (-not $SkipNPUCheck) {
        $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        if (-not $isAdmin) {
            Write-Warn "Not running as Administrator. NPU driver detection may be limited."
            $setupCmd = "Start-Process powershell -Verb RunAs -ArgumentList '-ExecutionPolicy Bypass -File `"$PSCommandPath`"'"
            Write-Warn "To enable full NPU support, re-run: $setupCmd"
        }
    }

    # Git and curl availability
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Write-Error "Git is required but not installed. Install from: https://git-scm.com/download/win"
    }
    if (-not (Get-Command curl -ErrorAction SilentlyContinue)) {
        Write-Error "curl is required but not installed. Enable via: Settings > Apps > Optional Features > Add 'curl'"
    }

    Write-Success "System checks passed"
}

# ========================================
# Platform Detection
# ========================================
function Get-PlatformInfo {
    $arch = (Get-CimInstance Win32_Processor).Architecture
    $platform = switch ($arch) {
        0 { "x86" }      # x86
        9 { "x86_64" }   # x64
        12 { "arm64" }   # ARM64
        default {
            Write-Warn "Unknown architecture: $arch. Defaulting to x86_64"
            "x86_64"
        }
    }

    return @{
        Architecture = $platform
        OS = "pc-windows-msvc"
        Target = "$platform-pc-windows-msvc"
    }
}

# ========================================
# Download & Install Binaries
# ========================================
function Install-Binaries {
    param($PlatformInfo)

    Write-Info "Fetching latest release from $REPO..."

    try {
        $release = Invoke-RestMethod -Uri $RELEASE_ENDPOINT -Headers @{"Accept"="application/vnd.github.v3+json"}
        $tag = $release.tag_name
        $assets = $release.assets
    } catch {
        Write-Warn "No releases found (repo may not have a release yet)."
        Write-Warn "Falling back to source build..."
        Build-FromSource
        return
    }

    if (-not $tag -or $Local) {
        if ($Local) {
            Write-Info "Local install requested (--Local switch detected)."
        } else {
            Write-Warn "No release tag found. Falling back to source build..."
        }
        Build-FromSource -UseLocal $Local
        return
    }

    Write-Info "Installing HMIR $tag for $($PlatformInfo.Target)..."

    # Find matching asset
    $assetName = "hmir-$tag-$($PlatformInfo.Target).zip"
    $asset = $assets | Where-Object { $_.name -eq $assetName }

    if (-not $asset) {
        Write-Warn "Prebuilt binary not found: $assetName"
        Write-Warn "Falling back to source build (requires Rust toolchain)..."
        Build-FromSource
        return
    }

    # Create temp directory
    $tempDir = Join-Path $env:TEMP "hmir-install-$((Get-Date).ToString('yyyyMMddHHmmss'))"
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    try {
        # Download and extract
        $downloadPath = Join-Path $tempDir $assetName
        Write-Info "Downloading $($asset.browser_download_url)..."
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $downloadPath

        Write-Info "Extracting to $tempDir..."
        Expand-Archive -Path $downloadPath -DestinationPath $tempDir -Force

        # Create install directory
        if (-not (Test-Path $InstallPath)) {
            New-Item -ItemType Directory -Path $InstallPath -Force | Out-Null
        }

        # Copy binaries
        Get-ChildItem "$tempDir\hmir-*" -File | ForEach-Object {
            Copy-Item $_.FullName -Destination $InstallPath -Force
            Write-Info "Installed $($_.Name)"
        }

        # Make executables
        Get-ChildItem "$InstallPath\*.exe" | ForEach-Object {
            Unblock-File $_.FullName  # Remove Mark of the Web
        }

        Write-Success "Binaries installed to $InstallPath"

    } finally {
        # Cleanup temp
        if (Test-Path $tempDir) {
            Remove-Item -Recurse -Force $tempDir
        }
    }
}

# ========================================
# Fallback: Build from Source
# ========================================
function Build-FromSource {
    param([switch]$UseLocal)

    # Check Rust toolchain
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "[!] Rust toolchain required for source build." -ForegroundColor $ColorError
        Write-Host "    Install via: winget install Rustlang.Rustup" -ForegroundColor $ColorError
        Write-Host "    Or visit: https://rustup.rs" -ForegroundColor $ColorError
        return
    }

    $tempRepo = $null
    $sourcePath = $null

    if ($UseLocal -or (Test-Path "$PSScriptRoot\..\Cargo.toml") -or (Test-Path ".\Cargo.toml")) {
        if (Test-Path ".\Cargo.toml") {
            $sourcePath = Get-Location
        } elseif (Test-Path "$PSScriptRoot\..\Cargo.toml") {
            $sourcePath = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent # Simplified path resolution
            $sourcePath = Resolve-Path "$PSScriptRoot\.."
        }

        if ($sourcePath) {
            Write-Success "Detected HMIR source at $sourcePath. Building local version..."
        }
    }

    if (-not $sourcePath) {
        Write-Warn "Building HMIR from source (this may take 20-45 minutes)..."
        # Clone repo to temp
        $tempRepo = Join-Path $env:TEMP "hmir-source-$((Get-Date).ToString('yyyyMMddHHmmss'))"
        Write-Info "Cloning repository to $tempRepo..."
        git clone --depth 1 --branch main "https://github.com/$REPO.git" $tempRepo | Out-Null
        $sourcePath = $tempRepo
    }

    try {
        Push-Location $sourcePath

        # Build Web UI static assets
        if (Test-Path "hmir-api\src\build_ui.ps1") {
            Write-Info "Building Web UI static assets..."
            powershell -ExecutionPolicy Bypass -File "hmir-api\src\build_ui.ps1"
        }

        # Build release (no --features on virtual workspace manifest)
        Write-Info "Building release binaries..."
        cargo build --release --workspace 2>&1 | Out-Host

        # Install to target path
        if (-not (Test-Path $InstallPath)) {
            New-Item -ItemType Directory -Path $InstallPath -Force | Out-Null
        }

        # Copy all hmir*.exe binaries (including hmir.exe and hmir-api.exe)
        $binaries = Get-ChildItem "target\release\hmir*.exe" -ErrorAction SilentlyContinue
        if ($binaries.Count -eq 0) {
            # Fallback specifically for hmir.exe if the wildcard fails
            $binaries = Get-ChildItem "target\release\hmir.exe" -ErrorAction SilentlyContinue
        }
        if ($binaries) {
            foreach ($bin in $binaries) {
                Copy-Item $bin.FullName -Destination $InstallPath -Force
                Write-Info "Installed $($bin.Name)"
            }

            # Copy scripts directory for NPU worker
            $srcScripts = Join-Path $sourcePath "scripts"
            if (Test-Path $srcScripts) {
                $destScripts = Join-Path $InstallPath "scripts"
                if (-not (Test-Path $destScripts)) {
                    New-Item -ItemType Directory -Path $destScripts -Force | Out-Null
                }
                Copy-Item -Path "$srcScripts\*" -Destination $destScripts -Force -Recurse
                Write-Success "NPU scripts installed to $destScripts"
            }

            Write-Success "Build complete. Binaries installed to $InstallPath"
        } else {
            Write-Warn "Build completed but no hmir-*.exe binaries found in target/release."
            Write-Warn "The workspace crates may not yet define [[bin]] targets."
        }

    } finally {
        Pop-Location
        if ($tempRepo -and (Test-Path $tempRepo)) {
            Remove-Item -Recurse -Force $tempRepo
        }
    }
}

# ========================================
# PATH Management
# ========================================
function Update-UserPath {
    if ($env:PATH -notlike "*$InstallPath*") {
        Write-Warn "$InstallPath is not in your user PATH."

        $confirm = Read-Host "Add $InstallPath to user PATH? [Y/n]"
        if ($confirm -eq "" -or $confirm -eq "Y" -or $confirm -eq "y") {
            # Use [Environment]::SetEnvironmentVariable for user-level PATH
            $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
            $newPath = "$InstallPath;$currentPath"
            [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")

            Write-Success "Added $InstallPath to user PATH"
            Write-Info "Restart PowerShell or run: `$env:PATH = $newPath; + `$env:PATH"
        }
    }
}

# ========================================
# NPU Driver Helper (Optional)
# ========================================
function Test-NPUDrivers {
    if ($SkipNPUCheck) { return $true }
    Write-Info "Checking for NPU hardware..."
    # Simple check for now to avoid nested blocks
    $devices = Get-PnpDevice -Class 'ComputeAccelerator' -ErrorAction SilentlyContinue
    if ($devices) {
        Write-Success "NPU hardware found."
        return $true
    } else {
        Write-Warn "No dedicated NPU hardware detected."
        return $false
    }
}

function Test-Installation {
    Write-Info "Verifying installation..."
    $env:PATH = "$InstallPath;$env:PATH"
    if (Get-Command hmir -ErrorAction SilentlyContinue) {
        Write-Success "HMIR command found."
    } else {
        Write-Warn "HMIR command not found."
    }
}

# ========================================
# Python Environment Setup
# ========================================
function Install-PythonEnvironment {
    Write-Info "Setting up Python virtual environment..."

    if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
        Write-Error "Python is required but not installed. Please install Python 3.10+."
    }

    $venvPath = Join-Path $InstallPath ".venv"
    if (-not (Test-Path $venvPath)) {
        Write-Info "Creating virtual environment at $venvPath..."
        python -m venv $venvPath
    } else {
        Write-Info "Virtual environment already exists at $venvPath."
    }

    $pip = Join-Path $venvPath "Scripts\pip.exe"
    if (-not (Test-Path $pip)) {
        Write-Error "Failed to locate pip in virtual environment."
    }

    Write-Info "Installing Python dependencies (aiohttp, openvino-genai, huggingface-hub)..."
    $python = Join-Path $venvPath "Scripts\python.exe"
    & $python -m pip install --upgrade pip | Out-Null
    & $pip install aiohttp openvino-genai huggingface-hub | Out-Null
    Write-Success "Python environment setup complete."
}

# ========================================
# Shortcuts & Shell Integration
# ========================================
function Create-DesktopShortcut {
    param (
        [string]$Target,
        [string]$Name,
        [string]$Arguments = ""
    )
    try {
        Write-Info "Creating desktop shortcut for $Name..."
        $WshShell = New-Object -ComObject WScript.Shell
        $ShortcutPath = [System.IO.Path]::Combine([Environment]::GetFolderPath("Desktop"), "$Name.lnk")
        $Shortcut = $WshShell.CreateShortcut($ShortcutPath)
        $Shortcut.TargetPath = $Target
        $Shortcut.Arguments = $Arguments
        $Shortcut.WorkingDirectory = $InstallPath
        $Shortcut.Description = "Launch HMIR ELITE $Name"
        # If hmir.exe is used as target, it might not have a good icon, 
        # but for now we just save it.
        $Shortcut.Save()
        Write-Success "Desktop shortcut created: $Name.lnk"
    } catch {
        Write-Warn "Failed to create desktop shortcut: $_"
    }
}

# ========================================
# Main Execution
# ========================================
function Main {
    Write-Host " HMIR Windows Installer | Build: $BUILD_TIMESTAMP" -ForegroundColor $ColorInfo
    Write-Host "Repository: https://github.com/$REPO" -ForegroundColor $ColorInfo
    Write-Host ""

    if ($DryRun) {
        Write-Host " Dry-run mode: showing actions without applying" -ForegroundColor $ColorWarn
        Write-Host "Target install path: $InstallPath"
        return
    }

    Test-SystemRequirements
    Invoke-ForcePurge

    $platform = Get-PlatformInfo
    Write-Info "Detected platform: $($platform.Target)"

    Install-Binaries -PlatformInfo $platform
    Install-PythonEnvironment
    Update-UserPath
    
    # One-click Visual: Create Desktop Shortcut
    Create-DesktopShortcut -Target "$InstallPath\hmir.exe" -Name "HMIR Dashboard" -Arguments "start"
    
    $npuDetected = Test-NPUDrivers
    if (-not $npuDetected) {
        Write-Warn "NPU not detected or drivers inactive. AI inference will fall back to CPU."
    }
    
    # Verification
    if (-not (Test-Path "$InstallPath\hmir.exe")) {
        Write-Error "CRITICAL: hmir.exe not found in $InstallPath after installation."
        Read-Host "Press Enter to close..."
        return
    }
    
    Test-Installation

    Write-Host ""
    Write-Success " Installation complete!"
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor $ColorInfo
    Write-Host "  1. Restart PowerShell or run: `$env:PATH = '$InstallPath;' + `$env:PATH"
    Write-Host "  2. Get model recommendations: hmir suggest"
    Write-Host "  3. Start native dashboard: hmir start"
    Write-Host "  4. Start legacy web API UI: hmir start --web"
    Write-Host "  5. Integration help: hmir integrations"
    Write-Host "  6. API endpoint: http://localhost:$API_PORT/v1/chat/completions"
    Write-Host ""
    Write-Host "Documentation: https://github.com/$REPO/blob/main/README.md" -ForegroundColor $ColorInfo
    Write-Host "Troubleshooting: hmir logs --tail 200"
    Write-Host ""
    Read-Host "Installation process complete. Press Enter to close this window..."
}

# Run main
Main
# End of HMIR Installer
