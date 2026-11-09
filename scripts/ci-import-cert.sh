#!/usr/bin/env bash
# CI only: puts the signing certificate from the TYPIO_SIGN_P12 (base64 .p12) and
# TYPIO_SIGN_P12_PASSWORD secrets in a temporary keychain and sets TYPIO_SIGN_IDENTITY for
# the following steps. Without the secrets (such as pull requests from forks) builds are
# signed ad hoc.
set -euo pipefail

if [ -z "${TYPIO_SIGN_P12:-}" ] || [ -z "${TYPIO_SIGN_P12_PASSWORD:-}" ]; then
  echo "No signing certificate in the secrets; the app is signed ad hoc."
  exit 0
fi

dir="${RUNNER_TEMP:?}"
keychain="$dir/typio-signing.keychain-db"
keychain_password="$(uuidgen)"
p12="$dir/typio-signing.p12"
trap 'rm -f "$p12" "$dir/typio-signing.cer"' EXIT

printf '%s' "$TYPIO_SIGN_P12" | base64 --decode > "$p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$p12" -k "$keychain" -P "$TYPIO_SIGN_P12_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

# codesign only uses certificates trusted for code signing.
/usr/bin/openssl pkcs12 -in "$p12" -nokeys -clcerts -passin env:TYPIO_SIGN_P12_PASSWORD \
  -out "$dir/typio-signing.cer"
sudo security add-trusted-cert -d -r trustRoot -p codeSign -k "$keychain" "$dir/typio-signing.cer"

identity="$(security find-identity -v -p codesigning "$keychain" | awk 'NR == 1 { print $2 }')"
[ -n "$identity" ] || { echo "The certificate cannot sign code." >&2; exit 1; }
echo "TYPIO_SIGN_IDENTITY=$identity" >> "$GITHUB_ENV"
echo "Signing with the certificate from the secrets."
