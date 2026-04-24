# Assembles ui_shell.html + ui_css.css + ui_js.js into index.html
$dir = Split-Path -Parent $MyInvocation.MyCommand.Path
$css = Get-Content (Join-Path $dir "ui_css.css") -Encoding UTF8 -Raw
$js = Get-Content (Join-Path $dir "ui_js.js") -Encoding UTF8 -Raw
$html = Get-Content (Join-Path $dir "ui_shell.html") -Encoding UTF8 -Raw
$html = $html.Replace("/* HMIR_CSS_PLACEHOLDER */", $css)
$html = $html.Replace("/* HMIR_JS_PLACEHOLDER */", $js)
Set-Content -Path (Join-Path $dir "index.html") -Value $html -Encoding UTF8
Write-Host "Assembled index.html ($($html.Length) bytes)"
