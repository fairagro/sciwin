$ErrorActionPreference = "Stop"

$downloadUrl = "https://github.com/fairagro/sciwin/releases/latest/download/s4n-installer.ps1"

powershell -ExecutionPolicy Bypass -c "irm $downloadUrl | iex"