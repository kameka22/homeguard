#!/bin/sh
# HomeGuard — installateur agent (Linux).
#
# TEMPLATE : ce fichier est destiné à être servi par l'app (section 4 du
# PLAN.md), qui substitue les placeholders __EN_MAJUSCULES__ avant de le
# transmettre. Non fonctionnel tel quel — c'est le module app-core qui
# reste à écrire pour le servir (voir le rapport de session).
#
# Usage attendu, une fois servi par l'app :
#   curl -fsSL http://<ip-app>:7880/install.sh?t=<TOKEN> | sudo sh

set -eu

AGENT_URL_X86_64="__AGENT_URL_LINUX_X86_64__"
AGENT_URL_ARM64="__AGENT_URL_LINUX_ARM64__"
AGENT_SHA256_X86_64="__AGENT_SHA256_LINUX_X86_64__"
AGENT_SHA256_ARM64="__AGENT_SHA256_LINUX_ARM64__"
ENROLL_URL="__ENROLL_URL__"
TOKEN="__TOKEN__"
CA_SHA256="__CA_SHA256__"

BIN_PATH="/usr/local/bin/homeguard-agent"

if [ "$(id -u)" -ne 0 ]; then
    echo "Ce script doit être exécuté en root (sudo)." >&2
    exit 1
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        AGENT_URL="$AGENT_URL_X86_64"
        AGENT_SHA256="$AGENT_SHA256_X86_64"
        ;;
    aarch64|arm64)
        AGENT_URL="$AGENT_URL_ARM64"
        AGENT_SHA256="$AGENT_SHA256_ARM64"
        ;;
    *)
        echo "Architecture non prise en charge : $ARCH" >&2
        exit 1
        ;;
esac

TMP_BIN="$(mktemp)"
trap 'rm -f "$TMP_BIN"' EXIT

echo "Téléchargement de l'agent…"
curl -fsSL "$AGENT_URL" -o "$TMP_BIN"

echo "Vérification de l'intégrité…"
COMPUTED_SHA256="$(sha256sum "$TMP_BIN" | cut -d' ' -f1)"
if [ "$COMPUTED_SHA256" != "$AGENT_SHA256" ]; then
    echo "Échec de la vérification SHA-256 (attendu $AGENT_SHA256, obtenu $COMPUTED_SHA256)." >&2
    exit 1
fi

install -m 0755 "$TMP_BIN" "$BIN_PATH"

echo "Enrôlement auprès de l'application…"
"$BIN_PATH" enroll --url "$ENROLL_URL" --token "$TOKEN" --ca-sha256 "$CA_SHA256"

echo "Installation du service…"
"$BIN_PATH" install

echo "Terminé. L'agent tourne en arrière-plan et démarre avec le système."
