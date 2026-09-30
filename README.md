# HomeGuard

Gestion centralisée des sites bloqués sur les postes Windows/Linux d'un réseau
domestique, pilotée depuis une application desktop macOS.

Un agent léger tourne comme service système sur chaque poste et applique un
blocage via le fichier `hosts` — horaires, déblocages temporaires,
auto-réparation en cas d'altération, et neutralisation du contournement par
DNS-over-HTTPS. L'application macOS ne fait jamais tourner les agents en
arrière-plan côté serveur : elle se connecte à eux uniquement quand elle est
lancée ou sur action explicite (modèle *push*, pas de polling).

> Projet personnel, développé pour un usage familial. Document d'architecture
> complet et vivant : [PLAN.md](./PLAN.md).

## Fonctionnalités

- **Listes réutilisables** de domaines, affectées à un poste ou un groupe de
  postes, avec expansion automatique des variantes courantes (`www.`, `m.`,
  domaines alternatifs).
- **Planification horaire** par affectation (grille semaine), évaluée dans un
  fuseau fixe côté agent — pas le fuseau système, pour éviter tout
  contournement.
- **Déblocages temporaires** (15 min → jusqu'au soir), retirés automatiquement
  à l'échéance même si l'application est fermée.
- **Auto-réparation** : le fichier `hosts` est surveillé, toute altération est
  corrigée et journalisée.
- **Neutralisation du DNS-over-HTTPS** (Chrome, Edge, Brave, Chromium,
  Firefox) pour empêcher le contournement du blocage par DoH.
- **Découverte automatique** des postes sur le réseau local (mDNS) — aucune
  IP à saisir à la main.
- **Déploiement en une commande** (`curl | sudo sh` / `irm | iex`), binaire
  agent embarqué dans l'application, vérifié par empreinte SHA-256.
- **Mise à jour à distance** des agents, avec vérification post-redémarrage
  (par poste ou en une seule action groupée).
- **Statut réel en ligne/hors ligne** de chaque poste, pas seulement « déjà vu
  un jour ».
- **Tableau de bord** : écarts détectés, prochaines transitions d'horaire,
  historique unifié app + agents.
- **Export / import chiffré** de toute la configuration (y compris la CA),
  pour migrer vers un autre Mac sans réenrôler les postes.
- **Sécurité** : mTLS de bout en bout, CA auto-générée et stockée dans le
  Trousseau macOS, aucune clé privée d'agent ne quitte jamais son poste.

## Architecture

```
┌────────────── Mac (app Tauri) ───────────────┐
│  UI (Svelte)                                 │
│  Backend Rust                                │
│   ├─ PKI (rcgen) ── CA + cert client         │──► Trousseau macOS
│   ├─ Découverte mDNS (mdns-sd)               │
│   ├─ Client mTLS (reqwest + rustls)          │
│   ├─ Moteur de politique                     │
│   ├─ Synchronisation / historique            │
│   ├─ Serveur d'enrôlement (axum, temporaire) │
│   └─ Stockage local (SQLite)                 │
└───────────────┬──────────────────────────────┘
                │  mTLS :7878         ▲ mDNS _homeguard._tcp
                ▼                     │
┌──────── Poste Windows / Linux ──────┴────────┐
│  homeguard-agent (service SYSTEM/root)       │
│   ├─ API HTTPS (axum + rustls, mTLS)         │
│   ├─ Annonce mDNS                            │
│   ├─ Évaluateur de politique                 │
│   ├─ Écriture bloc hosts + flush DNS         │
│   └─ Journal d'événements local              │
└──────────────────────────────────────────────┘
```

Le diagramme complet, le modèle de menace et le détail de chaque flux sont
dans [PLAN.md](./PLAN.md).

## Structure du dépôt

```
homeguard/
├── crates/
│   ├── common/      # modèles partagés : politique, parsing du hosts, PKI, types d'API
│   ├── agent/       # binaire agent (run / enroll / install / uninstall), service Windows/Linux
│   └── app-core/    # logique côté app : SQLite, moteur de politique, client mTLS, mDNS, serveurs HTTP
├── app/             # application Tauri (macOS uniquement)
│   ├── src/         # frontend Svelte
│   └── src-tauri/   # backend Rust, commandes exposées à l'UI
├── scripts/         # build-agents.sh, install.sh, install.ps1
└── PLAN.md          # document d'architecture et d'avancement
```

## Prérequis

- **macOS** (l'application de pilotage est macOS uniquement)
- [Rust](https://rustup.rs/) (édition 2021, toolchain stable)
- [Node.js](https://nodejs.org/) + npm
- Pour compiler les binaires agent Windows/Linux depuis le Mac :
  [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild), `zig` et
  `mingw-w64` (voir l'en-tête de [`scripts/build-agents.sh`](./scripts/build-agents.sh)
  pour les commandes d'installation exactes)

## Démarrage

```bash
git clone git@github.com:kameka22/homeguard.git
cd homeguard

# Compile les binaires agent (Linux x86_64/arm64, Windows x86_64) et les
# dépose dans app/src-tauri/resources/agents/ — nécessaire avant le premier
# lancement de l'app pour que l'installateur en une commande fonctionne.
./scripts/build-agents.sh

cd app
npm install

# Lancement en mode développement (rechargement à chaud)
npm run tauri dev
```

## Build de production

```bash
cd app
npm run tauri build
```

Produit un `.app` et un `.dmg` dans `app/src-tauri/target/release/bundle/`.

## Tests

```bash
# Depuis la racine du dépôt — common, agent, app-core
cargo test --workspace
```

## Utilisation

1. Lancer HomeGuard sur le Mac ; au premier lancement, une CA locale est
   générée et stockée dans le Trousseau.
2. Depuis l'écran Postes, cliquer sur **+ Postes** : une commande à copier
   s'affiche (IP, jeton et empreinte de CA déjà remplis).
3. Exécuter cette commande sur le poste à surveiller (PowerShell administrateur
   sous Windows, `sudo sh` sous Linux). L'agent se télécharge, vérifie son
   intégrité, s'enrôle auprès de l'app et s'installe comme service système.
4. Le poste apparaît automatiquement dans l'application (découverte mDNS).

## État d'avancement

Le projet est fonctionnellement complet côté code. Développé et testé
principalement en environnement Linux/macOS ; la partie service Windows
compile (cross-compilation vérifiée) mais n'a pas encore été validée sur une
machine Windows réelle.

| Domaine | État |
|---|---|
| Modèle de politique, PKI, parsing hosts (`common`) | ✅ testé |
| Agent — cœur (API, auto-réparation, anti-DoH, service Linux) | ✅ testé |
| Agent — service Windows (SCM + dispatcher) | 🟡 compile, non validé sur machine réelle |
| Enrôlement et PKI assistée | ✅ testé |
| Application Tauri (macOS, Trousseau, mDNS, design) | ✅ validé sur macOS |
| Déploiement (scripts d'install, compilation croisée) | ✅ vérifié |
| Planification, déblocages temporaires | ✅ testé |
| Intégrité et anti-DoH Windows | 🟡 écrit, non vérifié sur machine réelle |
| Mise à jour à distance des agents | ✅ testé (mécanisme), non exercé en conditions réelles |
| Export / import chiffré | ✅ testé |
| Signature et notarisation macOS | ⬜ à faire |

Détail complet, section par section : [PLAN.md](./PLAN.md).

## Sécurité et limites

- Toute communication entre l'app et les agents passe en mTLS : chacun ne
  parle qu'à une partie présentant un certificat signé par la CA locale.
- La clé privée de chaque agent est générée sur son propre poste et ne le
  quitte jamais.
- Le blocage agit sur le fichier `hosts` et les politiques navigateur listées
  ci-dessus ; il ne couvre pas les VPN, proxys, ou navigateurs portables non
  gérés.
- Les scripts d'installation sont servis en HTTP simple sur le réseau local
  (attaquable par un tiers sur le même LAN) — l'intégrité du binaire est
  vérifiée par SHA-256 et l'enrôlement par empreinte de CA, ce qui reste
  acceptable sur un réseau domestique maîtrisé.

## Licence

[MIT](./LICENSE)
