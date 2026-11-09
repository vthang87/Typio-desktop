#!/usr/bin/env bash
# Creates the "Typio Self-Signed" code signing certificate, once. Every build signed with it
# keeps the same signature, so macOS keeps Accessibility access across updates.
# It adds the certificate to your login keychain (macOS asks for your password to trust it
# for code signing) and saves it as a .p12 file for the GitHub secrets that CI signs with.
# Keep the .p12 safe: a new certificate means everyone grants Accessibility again.
#
# Usage: scripts/make-signing-cert.sh [output folder, default ~/.typio-signing]
set -euo pipefail

name="Typio Self-Signed"
out="${1:-$HOME/.typio-signing}"
p12="$out/typio-signing.p12"
keychain="$HOME/Library/Keychains/login.keychain-db"

if [ -e "$p12" ]; then
  echo "$p12 already exists. Delete it first to make a new certificate." >&2
  exit 1
fi
mkdir -p "$out"
chmod 700 "$out"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cat > "$work/cert.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $name
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/cert.cnf" \
  -keyout "$work/key.pem" -out "$out/typio-signing.cer" 2>/dev/null

read -r -s -p "Password for the .p12 file: " password; echo
read -r -s -p "Repeat it: " again; echo
[ "$password" = "$again" ] || { echo "The passwords differ." >&2; exit 1; }
[ -n "$password" ] || { echo "The password is empty." >&2; exit 1; }
TYPIO_P12_PASSWORD="$password" /usr/bin/openssl pkcs12 -export -name "$name" \
  -inkey "$work/key.pem" -in "$out/typio-signing.cer" -out "$p12" -passout env:TYPIO_P12_PASSWORD
chmod 600 "$p12"

security import "$p12" -k "$keychain" -P "$password" -T /usr/bin/codesign
echo "macOS will ask for your password to trust the certificate for code signing."
security add-trusted-cert -r trustRoot -p codeSign -k "$keychain" "$out/typio-signing.cer"

echo
echo "Created $p12; scripts/bundle-macos.sh now signs with \"$name\"."
echo "To let CI sign the same way, set two repository secrets, for example:"
echo "  base64 -i \"$p12\" | gh secret set TYPIO_SIGN_P12 --repo vthang87/Typio-desktop"
echo "  gh secret set TYPIO_SIGN_P12_PASSWORD --repo vthang87/Typio-desktop"
