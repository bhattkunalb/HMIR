
$content = Get-Content "scripts/install.ps1"
$cleaned = $content | ForEach-Object { $_.TrimEnd() }
$cleaned | Set-Content "scripts/install.ps1" -Encoding UTF8
