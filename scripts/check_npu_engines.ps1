# Check what GPU/NPU adapters are visible to Windows
Write-Host "=== GPU Adapters (Win32_VideoController) ==="
Get-CimInstance Win32_VideoController | Select-Object Name, PNPDeviceID | Format-Table -AutoSize

Write-Host "`n=== GPU Adapter Memory ==="
Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapterMemory | Select-Object Name -First 5 | Format-Table -AutoSize

Write-Host "`n=== NPU/Accelerator Devices ==="
Get-PnpDevice -Class 'ComputeAccelerator' -ErrorAction SilentlyContinue | Select-Object FriendlyName, Status, InstanceId | Format-Table -AutoSize

Write-Host "`n=== Unique LUIDs from GPU engines ==="
$engines = Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine
$luids = @{}
foreach ($e in $engines) {
    $parts = $e.Name -split '_'
    $luid = $parts[3]
    if (-not $luids.ContainsKey($luid)) {
        $luids[$luid] = @()
    }
    $engtype = ($e.Name -split 'engtype_')[-1]
    if ($engtype -and -not ($luids[$luid] -contains $engtype)) {
        $luids[$luid] += $engtype
    }
}
foreach ($key in $luids.Keys) {
    $types = ($luids[$key] | Sort-Object -Unique) -join ', '
    Write-Host "LUID $key -> Engine Types: $types"
}
