# cleanup_certs.ps1
# Please run as Administrator

$CertName = "PlatformPasserDev"
Write-Host "Cleaning up certificate: $CertName" -ForegroundColor Cyan

$Stores = @(
    "Cert:\CurrentUser\My",
    "Cert:\CurrentUser\Root",
    "Cert:\LocalMachine\Root"
)

foreach ($StorePath in $Stores) {
    if (Test-Path $StorePath) {
        $certs = Get-ChildItem -Path $StorePath | Where-Object { $_.Subject -eq "CN=$CertName" }
        if ($certs) {
            foreach ($cert in $certs) {
                Write-Host "Removing cert from $StorePath : $($cert.Thumbprint)" -ForegroundColor Yellow
                Remove-Item -Path "$StorePath\$($cert.Thumbprint)" -Force
            }
        }
    }
}

Write-Host "Cleanup completed successfully." -ForegroundColor Green
