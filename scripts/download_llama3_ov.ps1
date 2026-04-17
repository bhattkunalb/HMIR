# ── HMIR NPU Model Downloader (PowerShell) ──────────────────────────
# Downloads OpenVINO-optimized models from Hugging Face.
# Default: Qwen2.5-1.5B-Instruct-INT4-OV

param(
    [string]$Model = "qwen2.5-1.5b-instruct-int4-ov",
    [string]$RepoBase = "https://huggingface.co/OpenVINO"
)

$modelDir = "$env:LOCALAPPDATA\hmir\models\$Model"
if (!(Test-Path $modelDir)) { New-Item -ItemType Directory -Path $modelDir -Force | Out-Null }

$repoUrl = "$RepoBase/$Model/resolve/main"
$files = @(
    "openvino_model.xml",
    "openvino_model.bin",
    "openvino_tokenizer.xml",
    "openvino_tokenizer.bin",
    "openvino_detokenizer.xml",
    "openvino_detokenizer.bin",
    "tokenizer.json",
    "tokenizer_config.json",
    "config.json",
    "special_tokens_map.json",
    "generation_config.json"
)

$headers = @{ "User-Agent" = "HMIR-NPU-Sync/2.0" }

Write-Host "Syncing $Model to $modelDir ..." -ForegroundColor Cyan

foreach ($file in $files) {
    $targetPath = Join-Path $modelDir $file
    $remoteUrl = "$repoUrl/$file`?download=true"

    if (Test-Path $targetPath) {
        $size = (Get-Item $targetPath).Length
        if ($size -gt 1024) {
            Write-Host "  $file already exists ($size bytes), skipping..." -ForegroundColor DarkGray
            continue
        }
    }

    Write-Host "  Downloading $file ..." -NoNewline
    try {
        Invoke-WebRequest -Uri $remoteUrl -OutFile $targetPath -Headers $headers -ErrorAction Stop
        Write-Host " OK" -ForegroundColor Green
    } catch {
        Write-Warning "  Failed to download $file (may not exist in this model)"
    }
}

Write-Host "`n$Model NPU model sync complete." -ForegroundColor Green
Write-Host "Location: $modelDir"
