# scripts/install.ps1
# One-click HMIR installer for Windows 10/11 (PowerShell)
# Usage: irm https://raw.githubusercontent.com/bhattkunalb/HMIR/main/scripts/install.ps1 | iex
# Note: Run PowerShell as Administrator for NPU driver access (optional)

param(
    [switch]$DryRun,
    [switch]$SkipNPUCheck,
    [string]$InstallPath = "$env:USERPROFILE\.hmir"
)

# ========================================
# Configuration & Constants
# ========================================
$REPO = "bhattkunalb/HMIR"
$RELEASE_ENDPOINT = "https://api.github.com/repos/$REPO/releases/latest"
$API_PORT = 8080
$DASHBOARD_PORT = 3001
$MIN_WINDOWS_BUILD = 19041  # Windows 10 20H2
$REQUIRED_NET_VERSION = "6.0"  # .NET 6+ for some dependencies

# Colors for output
$ColorInfo = "Cyan"
$ColorSuccess = "Green"
$ColorWarn = "Yellow"
$ColorError = "Red"

function Write-Info    { param($msg) Write-Host "[INFO] " -ForegroundColor $ColorInfo -NoNewline; Write-Host $msg }
function Write-Success { param($msg) Write-Host "[✓] " -ForegroundColor $ColorSuccess -NoNewline; Write-Host $msg }
function Write-Warn    { param($msg) Write-Host "[⚠] " -ForegroundColor $ColorWarn -NoNewline; Write-Host $msg }
function Write-Error   { param($msg) Write-Host "[✗] " -ForegroundColor $ColorError -NoNewline; Write-Host $msg; exit 1 }

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
            Write-Warn "To enable full NPU support, re-run: Start-Process powershell -Verb RunAs -ArgumentList '-ExecutionPolicy Bypass -File `"$PSCommandPath`"'"
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
    
    if (-not $tag) {
        Write-Warn "No release tag found. Falling back to source build..."
        Build-FromSource
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
    Write-Warn "Building HMIR from source (this may take 20-45 minutes)..."
    
    # Check Rust toolchain
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Error "Rust toolchain required for source build."
        Write-Error "Install via: winget install Rustlang.Rustup"
        Write-Error "Or visit: https://rustup.rs"
    }
    
    # Clone repo to temp
    $tempRepo = Join-Path $env:TEMP "hmir-source-$((Get-Date).ToString('yyyyMMddHHmmss'))"
    Write-Info "Cloning repository to $tempRepo..."
    git clone --depth 1 --branch main "https://github.com/$REPO.git" $tempRepo | Out-Null
    
    try {
        Push-Location $tempRepo
        
        # Build release
        Write-Info "Building release binaries..."
        cargo build --release --workspace --features dashboard,openai-api,hardware-prober
        
        # Install to target path
        if (-not (Test-Path $InstallPath)) {
            New-Item -ItemType Directory -Path $InstallPath -Force | Out-Null
        }
        
        Copy-Item "target\release\hmir-cli.exe" -Destination $InstallPath -Force
        Copy-Item "target\release\hmir-dashboard.exe" -Destination $InstallPath -Force
        
        Write-Success "Build complete. Binaries installed to $InstallPath"
        
    } finally {
        Pop-Location
        if (Test-Path $tempRepo) {
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
            Write-Info "Restart PowerShell or run: `$env:PATH = '$newPath' + `';`' + `$env:PATH"
        }
    }
}

# ========================================
# NPU Driver Helper (Optional)
# ========================================
function Test-NPUDrivers {
    if ($SkipNPUCheck) { return }
    
    Write-Info "Checking for NPU drivers..."
    
    $npuDetected = $false
    
    # Intel OpenVINO NPU
    if (Test-Path "C:\Program Files\Intel\OpenVINO") {
        Write-Success "Intel OpenVINO NPU runtime detected"
        $npuDetected = $true
    }
    
    # Qualcomm QNN (Snapdragon X Elite)
    if (Get-Command qnn-context-binary-generator -ErrorAction SilentlyContinue) {
        Write-Success "Qualcomm QNN runtime detected"
        $npuDetected = $true
    }
    
    # Windows ML / DirectML (fallback)
    if (Get-WindowsOptionalFeature -Online -FeatureName "Microsoft-Windows-Subsystem-Linux" -ErrorAction SilentlyContinue) {
        Write-Info "Windows ML / DirectML available as fallback backend"
    }
    
    if (-not $npuDetected) {
        Write-Warn "No dedicated NPU runtime detected. HMIR will auto-fallback to GPU/CPU."
        Write-Warn "For Snapdragon X Elite: Install Qualcomm AI Engine Direct drivers"
        Write-Warn "For Intel Core Ultra: Install OpenVINO 2024.0+"
    }
}

# ========================================
# Post-Install Verification
# ========================================
function Test-Installation {
    Write-Info "Verifying installation..."
    
    # Refresh PATH in current session
    $env:PATH = "$InstallPath;$env:PATH"
    
    if (Get-Command hmir -ErrorAction SilentlyContinue) {
        try {
            $version = hmir --version 2>$null
            Write-Success "HMIR installed: $version"
            
            # Quick hardware probe
            Write-Info "Probing hardware (first run may take 10s)..."
            $probe = hmir suggest --strategy latency 2>&1 | Select-Object -First 10
            $probe | ForEach-Object { Write-Host "  $_" }
            
        } catch {
            Write-Warn "hmir command found but execution failed: $_"
        }
    } else {
        Write-Warn "hmir command not found in current session."
        Write-Warn "Ensure $InstallPath is in PATH, or restart PowerShell."
    }
}

# ========================================
# Main Execution
# ========================================
function Main {
    Write-Host "🚀 HMIR Windows Installer" -ForegroundColor $ColorInfo
    Write-Host "Repository: https://github.com/$REPO" -ForegroundColor $ColorInfo
    Write-Host ""
    
    if ($DryRun) {
        Write-Host "🔍 Dry-run mode: showing actions without applying" -ForegroundColor $ColorWarn
        Write-Host "Target install path: $InstallPath"
        return
    }
    
    Test-SystemRequirements
    
    $platform = Get-PlatformInfo
    Write-Info "Detected platform: $($platform.Target)"
    
    Install-Binaries -PlatformInfo $platform
    Update-UserPath
    Test-NPUDrivers
    Test-Installation
    
    Write-Host ""
    Write-Success "🎉 Installation complete!"
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor $ColorInfo
    Write-Host "  1. Restart PowerShell or run: `$env:PATH = '$InstallPath;' + `$env:PATH"
    Write-Host "  2. Get model recommendations: hmir suggest"
    Write-Host "  3. Start the daemon: hmir start --dashboard"
    Write-Host "  4. Open dashboard: http://localhost:$DASHBOARD_PORT"
    Write-Host "  5. API endpoint: http://localhost:$API_PORT/v1/chat/completions"
    Write-Host ""
    Write-Host "Documentation: https://github.com/$REPO/blob/main/README.md" -ForegroundColor $ColorInfo
    Write-Host "Troubleshooting: hmir logs --level debug"
}

# Run main
Main
