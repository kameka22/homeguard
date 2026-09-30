# HomeGuard — installateur agent (Windows).
#
# TEMPLATE : ce fichier est destiné à être servi par l'app (section 4 du
# PLAN.md), qui substitue les placeholders __EN_MAJUSCULES__ avant de le
# transmettre. Non fonctionnel tel quel — c'est le module app-core qui
# reste à écrire pour le servir (voir le rapport de session).
#
# Usage attendu, une fois servi par l'app, dans PowerShell en administrateur :
#   irm http://<ip-app>:7880/install.ps1?t=<TOKEN> | iex

$ErrorActionPreference = "Stop"

$AgentUrl    = "__AGENT_URL_WINDOWS__"
$AgentSha256 = "__AGENT_SHA256_WINDOWS__"
$EnrollUrl   = "__ENROLL_URL__"
$Token       = "__TOKEN__"
$CaSha256    = "__CA_SHA256__"

$InstallDir = "$env:ProgramData\HomeGuard\bin"
$BinPath    = "$InstallDir\homeguard-agent.exe"

$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "Ce script doit être exécuté dans PowerShell en tant qu'administrateur."
    exit 1
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

Write-Host "Téléchargement de l'agent…"
$tmpBin = New-TemporaryFile
Invoke-WebRequest -Uri $AgentUrl -OutFile $tmpBin -UseBasicParsing

Write-Host "Vérification de l'intégrité…"
$computedSha256 = (Get-FileHash -Path $tmpBin -Algorithm SHA256).Hash
if ($computedSha256 -ne $AgentSha256.ToUpper()) {
    Write-Error "Échec de la vérification SHA-256 (attendu $AgentSha256, obtenu $computedSha256)."
    Remove-Item $tmpBin -Force
    exit 1
}

Move-Item -Force $tmpBin $BinPath

Write-Host "Ouverture du pare-feu (TCP 7878, UDP 5353)…"
New-NetFirewallRule -DisplayName "HomeGuard Agent (API)" -Direction Inbound -Protocol TCP -LocalPort 7878 -Action Allow -ErrorAction SilentlyContinue | Out-Null
New-NetFirewallRule -DisplayName "HomeGuard Agent (mDNS)" -Direction Inbound -Protocol UDP -LocalPort 5353 -Action Allow -ErrorAction SilentlyContinue | Out-Null

Write-Host "Enrôlement auprès de l'application…"
& $BinPath enroll --url $EnrollUrl --token $Token --ca-sha256 $CaSha256
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installation du service…"
& $BinPath install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Terminé. L'agent tourne en arrière-plan et démarre avec Windows."
