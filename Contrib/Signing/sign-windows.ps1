param([Parameter(Mandatory)][string] $Path)
$ErrorActionPreference = 'Stop'
if (-not $env:MAGICALCRYPTOWALLET_WINDOWS_CERTIFICATE -or -not $env:MAGICALCRYPTOWALLET_WINDOWS_CERTIFICATE_PASSWORD) {
    throw 'Configure the project Windows code-signing certificate before production packaging.'
}
$certificateBytes = [Convert]::FromBase64String($env:MAGICALCRYPTOWALLET_WINDOWS_CERTIFICATE)
$certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new(
    $certificateBytes, $env:MAGICALCRYPTOWALLET_WINDOWS_CERTIFICATE_PASSWORD,
    [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
try {
    if (-not $certificate.HasPrivateKey -or $certificate.NotAfter -lt [DateTime]::UtcNow) { throw 'Invalid production signing certificate.' }
    $target = Get-Item -LiteralPath $Path
    $files = if ($target.PSIsContainer) {
        Get-ChildItem -LiteralPath $target.FullName -File | Where-Object {
            $_.Name -like 'magicalcryptowallet*.exe' -or $_.Name -like 'MagicalCryptoWallet*.dll' -or $_.Name -eq 'libwabisabi.dll'
        }
    } else { @($target) }
    foreach ($file in $files) {
        $signature = Set-AuthenticodeSignature -FilePath $file.FullName -Certificate $certificate -HashAlgorithm SHA256 -TimestampServer 'http://timestamp.digicert.com'
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $certificate.Thumbprint) { throw 'Platform signature verification failed.' }
    }
} finally {
    $certificate.Dispose()
    [Array]::Clear($certificateBytes)
}
