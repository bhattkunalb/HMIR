# HMIR NPU Inference Test Tool (PowerShell)
# ────────────────────────────────────────────────────────────────────
# This script tests the local NPU worker running on http://127.0.0.1:8089

$Url = "http://127.0.0.1:8089/v1/chat/completions"
$Body = @{
    messages = @(
        @{ role = "user"; content = "What is the capital of France?" }
    )
} | ConvertTo-Json

Write-Host "📡 Sending request to HMIR NPU Worker..." -ForegroundColor Cyan

try {
    # Using Invoke-RestMethod for cleaner output parsing
    $Response = Invoke-RestMethod -Uri $Url -Method Post -Body $Body -ContentType "application/json"
    
    Write-Host "✅ NPU Response received!" -ForegroundColor Green
    Write-Host "AI: " -NoNewline
    $Response.choices[0].delta.content
} catch {
    Write-Error "❌ Failed to reach NPU worker. Ensure 'python scripts/hmir_npu_worker.py' is running."
}
