$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$installerUrl = "https://raw.githubusercontent.com/JsGuaman/HerdrMOD/master/website/install.ps1"
$manifestUrl = "https://github.com/JsGuaman/HerdrMOD/releases/latest/download/latest.json"
$tempInstaller = Join-Path ([System.IO.Path]::GetTempPath()) ("herdr-mod-install-" + [System.Guid]::NewGuid().ToString("N") + ".ps1")

try {
    Invoke-WebRequest -UseBasicParsing -Uri $installerUrl -OutFile $tempInstaller
    & $tempInstaller -Channel stable -ManifestUrl $manifestUrl
} finally {
    Remove-Item -LiteralPath $tempInstaller -Force -ErrorAction SilentlyContinue
}
