# sign_build.ps1

param (
    [string]$Target = ".\target\debug\platform-passer-cli.exe",
    [string]$CertName = "PlatformPasserDev"
)

# 1. Check if the certificate exists in the user's personal store
$cert = Get-ChildItem "Cert:\CurrentUser\My" | Where-Object { $_.Subject -eq "CN=$CertName" }

if (-not $cert) {
    Write-Host "Certificate '$CertName' not found. Creating a new self-signed code signing certificate..." -ForegroundColor Yellow
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=$CertName" -CertStoreLocation "Cert:\CurrentUser\My"
    Write-Host "Certificate created: $($cert.Thumbprint)" -ForegroundColor Green
} else {
    Write-Host "Found existing certificate: $($cert.Thumbprint)" -ForegroundColor Cyan
}

# 2. Check if the certificate is trusted (in Root store)
$rootCert = Get-ChildItem "Cert:\CurrentUser\Root" | Where-Object { $_.Thumbprint -eq $cert.Thumbprint }

if (-not $rootCert) {
    Write-Host "`nIMPORTANT: To support uiAccess='true', the certificate must be in the 'Trusted Root Certification Authorities' store." -ForegroundColor Yellow
    Write-Host "You will now see a prompt asking to install this certificate. Please click 'Yes'." -ForegroundColor Yellow
    
    # Export to temp file to re-import into Root
    $tmpFile = "$env:TEMP\$CertName.cer"
    Export-Certificate -Cert $cert -FilePath $tmpFile -Type CERT > $null
    
    try {
        # Import to Root (Requires user interaction for confirmation usually, or admin)
        Import-Certificate -FilePath $tmpFile -CertStoreLocation "Cert:\CurrentUser\Root" | Out-Null
        Write-Host "Certificate installed to Trusted Root." -ForegroundColor Green
    } catch {
        Write-Error "Failed to install certificate to Trusted Root. You may need to run this script as Administrator or manually import '$tmpFile' to Trusted Root."
    }
    Remove-Item $tmpFile -ErrorAction SilentlyContinue
} else {
    Write-Host "Certificate is already trusted." -ForegroundColor Cyan
}

# 3. Sign the executable
if (Test-Path $Target) {
    Write-Host "`nSigning '$Target'..." -ForegroundColor Cyan
    Set-AuthenticodeSignature -FilePath $Target -Certificate $cert
    
    $sig = Get-AuthenticodeSignature $Target
    if ($sig.Status -eq 'Valid') {
        Write-Host "Success! The executable is signed and valid." -ForegroundColor Green
        Write-Host "`n[NOTE] ONLY when you move '$Target' to a trusted directory (e.g. 'C:\Program Files\PlatformPasser\') will it be able to bypass UIPI." -ForegroundColor Magenta
    } else {
        Write-Error "Signing failed or signature is invalid. Status: $($sig.Status)"
        Write-Error "Message: $($sig.StatusMessage)"
    }
} else {
    Write-Error "Target file '$Target' not found. Did you build the project?"
}
