$modelDir = "$env:LOCALAPPDATA\hmir\models\Phi-3-mini-4k-instruct-int4-ov"
if (!(Test-Path $modelDir)) { New-Item -ItemType Directory -Path $modelDir -Force }

$repoUrl = "https://huggingface.co/OpenVINO/Phi-3-mini-4k-instruct-int4-ov/resolve/main"
$files = @(
    "openvino_model.xml",
    "openvino_model.bin",
    "tokenizer.json",
    "tokenizer_config.json",
    "config.json",
    "special_tokens_map.json"
)

foreach ($file in $files) {
    $targetPath = Join-Path $modelDir $file
    $remoteUrl = "$repoUrl/$file`?download=true"
    
    if (Test-Path $targetPath) {
        Write-Host "Checking $file..."
        $size = (Get-Item $targetPath).Length
        if ($size -gt 1024) { 
            Write-Host "$file already exists ($size bytes), skipping logic..."
            continue 
        }
    }
    
    Write-Host "Downloading $file from $repoUrl..."
    try {
        $headers = @{ "User-Agent" = "HMIR-NPU-Sync/1.0" }
        Invoke-WebRequest -Uri $remoteUrl -OutFile $targetPath -Headers $headers
        Write-Host "Successfully downloaded $file"
    } catch {
        Write-Error "Failed to download $file - $_"
    }
}

Write-Host "NPU Model Sync Complete."
