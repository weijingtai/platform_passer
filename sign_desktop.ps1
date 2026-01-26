# sign_desktop.ps1

param (
    [string]$Target = "C:\Program Files\platform-passer\platform-passer-desktop.exe",
    [string]$CertName = "PlatformPasserDev"
)

Write-Host "Target: $Target" -ForegroundColor Cyan

if (-not (Test-Path $Target)) {
    Write-Error "Target file not found at '$Target'."
    Write-Host "Please ensure you have RE-BUILT the app with 'cargo tauri build' and INSTALLED it." -ForegroundColor Yellow
    exit 1
}

# 1. Check/Create Cert
$cert = Get-ChildItem "Cert:\CurrentUser\My" | Where-Object { $_.Subject -eq "CN=$CertName" }
if (-not $cert) {
    Write-Host "Creating certificate '$CertName'..." -ForegroundColor Yellow
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=$CertName" -CertStoreLocation "Cert:\CurrentUser\My"
}

# 2. Trust Cert (Must be LocalMachine for uiAccess)
$rootCert = Get-ChildItem "Cert:\LocalMachine\Root" | Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
if (-not $rootCert) {
    Write-Host "Installing certificate to LOCAL MACHINE Trusted Root..." -ForegroundColor Yellow
    $tmpFile = "$env:TEMP\$CertName.cer"
    Export-Certificate -Cert $cert -FilePath $tmpFile -Type CERT > $null
    # Import to LocalMachine Root
    Import-Certificate -FilePath $tmpFile -CertStoreLocation "Cert:\LocalMachine\Root" | Out-Null
    Remove-Item $tmpFile -ErrorAction SilentlyContinue
}
else {
    Write-Host "Certificate is already in Local Machine Trusted Root." -ForegroundColor Cyan
}

# 3. Sign
Write-Host "Signing executable..." -ForegroundColor Cyan
Set-AuthenticodeSignature -FilePath $Target -Certificate $cert

$sig = Get-AuthenticodeSignature $Target
if ($sig.Status -eq 'Valid') {
    Write-Host "SUCCESS! The installed application is now signed." -ForegroundColor Green
    Write-Host "You can now run it directly from the desktop/start menu."
}
else {
    Write-Error "Signing failed. Status: $($sig.Status)"
    Write-Error "Ensure you are running this script as Administrator if the target is in C:\Program Files"
}
