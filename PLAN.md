# HomeGuard — Plan

Gestion centralisée des sites bloqués (fichier `hosts`) sur des postes Windows/Linux,
pilotée depuis une app desktop macOS (Rust + Tauri).

## 1. Principes

- **Push uniquement** : les agents ne contactent jamais l'app. Elle les interroge et les met à jour à son lancement ou sur action.
- **Agent autonome une fois configuré** : il applique seul horaires, déblocages temporaires et auto-réparation, sans l'app.
- **Agent = service système** : installé une fois en admin, il tourne au démarrage du poste pour toutes les sessions, y compris non admin.
- **Découverte automatique** : un agent installé s'annonce en mDNS → il apparaît dans l'app sans saisie.
- **Déploiement en une commande** copiée depuis l'app.
- **PKI invisible** : l'app génère sa CA et signe les certificats des agents à l'enrôlement. Aucune manipulation manuelle.
- **mTLS** partout : seul ton Mac peut parler aux agents, et l'app vérifie qu'elle parle à un vrai agent.
- **Agent simple, app intelligente** : l'app résout groupes, listes et variantes de domaines. Elle envoie à l'agent une **politique** déjà calculée.

## 2. Architecture

```
┌────────────── Mac (app Tauri) ───────────────┐
│  UI (Svelte)                                 │
│  Backend Rust                                │
│   ├─ PKI (rcgen) ── CA + cert client         │──► Trousseau macOS
│   ├─ Découverte mDNS (mdns-sd)               │
│   ├─ Client mTLS (reqwest + rustls)          │
│   ├─ Moteur de politique                     │
│   │    groupes + listes + horaires → par poste│
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
│   ├─ Évaluateur de politique (horaires,      │
│   │    déblocages temporaires)               │
│   ├─ Écriture bloc hosts + flush DNS         │
│   ├─ Gardien : surveille hosts + politiques  │
│   │    navigateurs, auto-réparation          │
│   └─ Journal d'événements local              │
└──────────────────────────────────────────────┘
```

Workspace Cargo :

```
homeguard/
├── crates/common/   # modèles, politique, évaluation des horaires, parsing hosts, API types
├── crates/agent/    # binaire agent (CLI install/uninstall/run)
└── app/             # Tauri (src-tauri + front)
```

## 3. Gestion des certificats (assistée)

### Premier lancement de l'app — 1 clic
1. L'app génère une **CA** (ECDSA P-256, 10 ans) et un **certificat client** signé par cette CA.
2. Les clés sont stockées dans le **Trousseau macOS** (`security-framework`), jamais sur disque en clair.
3. L'empreinte de la CA est affichée pour info. Bouton « Régénérer la CA » (⚠ oblige à ré-enrôler tous les postes).

### Enrôlement d'un poste (génération à la volée)
1. Dans l'app, « Ajouter des postes » :
   - démarre un **serveur d'enrôlement temporaire** (HTTPS :7879, cert signé par la CA)
   - génère un **jeton** (valable 30 min, utilisable par N postes)
   - affiche la commande à copier
   - option : groupe à attribuer d'office aux nouveaux postes
2. Sur le poste, l'installeur :
   - télécharge le binaire agent et vérifie son SHA-256
   - lance `homeguard-agent install --enroll https://<mac>:7879 --token <T> --ca-sha256 <FP>`
3. L'agent :
   - génère **sa propre clé localement**, qui ne quitte jamais le poste
   - envoie une CSR + le jeton ; il vérifie le serveur en épinglant l'empreinte de la CA (`--ca-sha256`)
   - reçoit son certificat signé (SAN = hostname + `hostname.local`, **jamais d'IP** — l'app résout le nom vers l'IP courante à chaque connexion, ce qui évite de re-signer si l'IP change), le cert CA et sa politique initiale
   - installe et démarre le service, ouvre le pare-feu (TCP 7878, UDP 5353)
4. L'app ferme le serveur d'enrôlement à la fermeture de la fenêtre ou à l'expiration du jeton.

### Confiance
- **Agent** : n'accepte que les clients présentant un cert signé par la CA.
- **App** : n'accepte que les agents présentant un cert signé par la CA. Pas de TOFU ni d'avertissement.
- **Retrait d'un poste** : `POST /v1/uninstall` (l'agent restaure le hosts et les politiques navigateurs, se désinstalle), puis le poste est ajouté à une liste de révocation locale (numéro de série).

## 4. Déploiement de l'agent

Commandes affichées par l'app (IP, jeton et empreinte pré-remplis, bouton copier) :

**Windows** (PowerShell admin) :
```powershell
irm http://192.168.1.20:7880/install.ps1?t=K7Q2-9XPA | iex
```

**Linux** :
```bash
curl -fsSL http://192.168.1.20:7880/install.sh?t=K7Q2-9XPA | sudo sh
```

- Les binaires agent sont **embarqués dans l'app** (ressources Tauri) et servis par elle. Pas de dépôt externe.
- Cibles : `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` (binaires statiques). Build depuis le Mac avec `cargo-zigbuild` / `cargo-xwin`.
- La même commande sert aussi à **réparer ou réinstaller** un agent.
- **L'agent est un service système, pas une application de session.** Installé une fois depuis une session admin, il est actif pour **toutes les sessions du poste**, admin ou non, et même avant toute connexion :
  - **Windows** : service `HomeGuardAgent` (crate `windows-service`), compte `LocalSystem`, démarrage `Automatic`. Il redémarre automatiquement en cas d'échec (`sc failure … actions= restart/5000`).
  - **Linux** : unité systemd `homeguard-agent.service`, `User=root`, `WantedBy=multi-user.target`, `Restart=always`.
  - Un utilisateur non admin ne peut ni arrêter le service, ni modifier le fichier hosts, ni lire la clé privée de l'agent (ACL/permissions `SYSTEM`/`root` uniquement, `0600`).
  - Fichiers de l'agent :
    - Windows : `C:\ProgramData\HomeGuard\`
    - Linux : `/etc/homeguard/` pour la config et les certs, `/var/lib/homeguard/` pour la politique et le journal, `/usr/local/bin/homeguard-agent` pour le binaire
  - Le fichier hosts étant global à la machine, le blocage s'applique à tous les utilisateurs.
- ⚠ Le script est servi en HTTP, donc attaquable par MITM sur le LAN. C'est acceptable sur un réseau maîtrisé : le binaire est vérifié par SHA-256 et l'enrôlement par empreinte de CA. Alternative : bouton « Exporter un paquet hors-ligne » (zip avec binaire + config) pour clé USB ou GPO.

## 5. Découverte et synchronisation

- L'agent annonce `_homeguard._tcp.local.` avec le TXT `id=<uuid>`, `os`, `ver` et `fp=<empreinte cert>`.
- Au lancement, l'app :
  1. parcourt mDNS pendant environ 3 s, et tente aussi les dernières IP connues
  2. interroge chaque agent en parallèle (`/v1/info`, `/v1/status`, `/v1/events?since=…`)
  3. **pousse automatiquement** la politique aux postes dont la version appliquée est en retard, par exemple parce qu'ils étaient éteints lors d'une modification. Option activée par défaut, désactivable.
- Les postes déjà connus mais absents sont affichés « hors ligne » avec leur dernier état connu (cache SQLite).
- Bouton ⟳ pour relancer un scan à tout moment.
- Limites :
  - mDNS ne traverse pas les VLAN/sous-réseaux. En secours : « Ajouter par IP », et on mémorise la dernière IP connue.
  - macOS 15+ : déclarer `NSLocalNetworkUsageDescription` + `NSBonjourServices` dans `Info.plist`.

## 6. Fonctionnalités

### 6.1 Gestion des listes
- **Listes réutilisables** : chaque liste est nommée (« Réseaux sociaux », « Jeux », « Streaming ») et contient des domaines.
  - Elle est **affectée** à un groupe ou à un poste, avec un horaire optionnel.
  - Une même liste peut être affectée à plusieurs cibles.
- **Groupes de postes** (« Enfants », « Salle info ») :
  - un poste peut appartenir à plusieurs groupes
  - sa politique effective = union des affectations de ses groupes + ses affectations directes
- **Listes privées uniquement** : elles sont créées et maintenues à la main dans l'app. Il n'y a pas d'import de listes publiques.
  - On peut coller plusieurs domaines d'un coup (un par ligne). L'app les normalise : elle retire `http(s)://` et les chemins, et élimine les doublons.
- **Variantes automatiques** :
  - un catalogue d'alias est embarqué dans l'app et modifiable (`aliases.toml`), par exemple `youtube.com` → `www.`, `m.`, `youtu.be`, `youtube-nocookie.com`
  - l'option `www.` s'applique par défaut
  - l'app affiche les variantes ajoutées et permet d'en exclure

### 6.2 Planification
- **Horaires par affectation** : « Jeux » est bloquée pour « Enfants » du lundi au vendredi, de 08:00 à 18:00.
  - L'horaire se définit sur une grille semaine au pas de 30 min.
  - Sans horaire, la liste est bloquée en permanence.
- **Évaluation par l'agent** :
  - L'agent calcule l'ensemble des domaines à bloquer :
    - au démarrage
    - à la réception d'une politique
    - à chaque transition d'horaire (timer sur la prochaine échéance)
    - toutes les 60 s par sécurité
  - Il ne réécrit le hosts que si le résultat change.
- **Fuseau figé** : les horaires sont évalués dans le fuseau de la politique (par ex. `Europe/Paris`, avec tzdb embarquée via `jiff`), pas dans le fuseau système.
  - Raison : sous Windows, un utilisateur standard **peut changer le fuseau horaire**, ce qui lui permettrait de contourner l'horaire.
  - Changer l'heure système, en revanche, exige les droits admin.
- **Déblocage temporaire** d'un domaine ou d'une liste entière sur un poste ou un groupe : 15 min, 30 min, 1 h, 2 h, ou « jusqu'à ce soir ».
  - Il est envoyé comme exception datée dans la politique.
  - L'agent le retire seul à l'échéance, même si l'app est fermée.
  - Les exceptions expirées sont nettoyées de la politique au scan suivant.

### 6.3 Contrôle et intégrité
- **Détection d'écart** : au scan, l'app compare pour chaque poste :
  - la version de politique attendue et la version appliquée
  - le hash du bloc hosts attendu et le hash réel
  - l'état des politiques navigateurs (DoH)
  Un poste en écart apparaît en ◐ orange, avec la cause.
- **Auto-réparation** (option par poste, activée par défaut) :
  - l'agent surveille le fichier hosts (crate `notify`, plus une vérification toutes les 60 s)
  - si le bloc est modifié ou supprimé, il le **réécrit** et journalise l'altération
  - les lignes **hors bloc** qui redéfinissent un domaine géré, par exemple `1.2.3.4 youtube.com`, sont commentées avec un marqueur `# [homeguard] désactivé:`
- **Neutralisation du DNS-over-HTTPS** : les navigateurs en DoH ignorent le fichier hosts. L'agent pose des politiques machine, les vérifie périodiquement et les restaure si besoin :

  | Navigateur | Windows (registre HKLM)                                        | Linux                                                          |
  |------------|----------------------------------------------------------------|----------------------------------------------------------------|
  | Chrome     | `SOFTWARE\Policies\Google\Chrome` `DnsOverHttpsMode=off`       | `/etc/opt/chrome/policies/managed/homeguard.json`            |
  | Edge       | `SOFTWARE\Policies\Microsoft\Edge` `DnsOverHttpsMode=off`      | `/etc/opt/edge/policies/managed/homeguard.json`              |
  | Brave      | `SOFTWARE\Policies\BraveSoftware\Brave` `DnsOverHttpsMode=off` | `/etc/brave/policies/managed/homeguard.json`                 |
  | Chromium   | `SOFTWARE\Policies\Chromium` `DnsOverHttpsMode=off`            | `/etc/chromium/policies/managed/homeguard.json`              |
  | Firefox    | `SOFTWARE\Policies\Mozilla\Firefox\DNSOverHTTPS` `Enabled=0`, `Locked=1` | `/etc/firefox/policies/policies.json` (fusion, sans écraser l'existant) |

  - Firefox installé en snap/flatpak utilise d'autres chemins, à gérer au cas par cas.
- **Arrêt de l'agent** : l'agent distingue un arrêt manuel du service d'un arrêt système (Windows : `SERVICE_CONTROL_STOP` vs `SHUTDOWN/PRESHUTDOWN` ; Linux : état `stopping` de systemd).
  - Un arrêt manuel est journalisé et remonte dans l'historique au scan suivant.
  - Un admin local peut toujours arrêter l'agent : on ne peut pas l'en empêcher, mais on le **voit**.
- **Hors périmètre** : VPN, proxys et navigateurs portables non gérés. Seuls les navigateurs listés ci-dessus sont pris en charge.

### 6.4 Visibilité
- **Historique** unifié, filtrable par poste, groupe, type et période, et exportable en CSV :
  - **actions de l'app** : modification de liste, affectation, application de politique, déblocage, mise à jour, enrôlement, retrait
  - **événements des agents** : démarrage, arrêt manuel, altération détectée/corrigée, politique DoH restaurée, transition d'horaire, mise à jour
  - Chaque agent garde un journal local (`events.jsonl`, rotation à 5 000 entrées). Chaque entrée porte un numéro de séquence, et l'app récupère les nouvelles entrées à chaque scan (`?since=<seq>`). Les événements survenus quand l'app est fermée ne sont donc pas perdus.
- **Tableau de bord** (écran d'accueil) :
  - tuiles : postes en ligne / total, postes en écart, altérations des dernières 24 h, agents obsolètes, déblocages actifs
  - « À traiter » : liste cliquable des postes à problème avec la cause
  - « Prochaines transitions » : ex. « Jeux se débloque à 18:00 sur 3 postes »

### 6.5 Maintenance
- **Mise à jour des agents depuis l'app** : quand la version embarquée dans l'app est plus récente, un bouton « Mettre à jour » s'affiche (par poste ou pour tous).
  1. L'app envoie le binaire (`POST /v1/update`, flux + SHA-256) à travers le canal mTLS.
  2. L'agent vérifie le binaire, remplace son exécutable (crate `self-replace`, compatible Windows) en gardant l'ancien en `.old`, puis sort. Le gestionnaire de service le relance.
  3. L'app vérifie la nouvelle version au scan suivant.
  4. Si l'agent ne revient pas, la commande d'installation sert à le réparer.
- **Nom et étiquettes** : nom d'affichage (« PC de Léo ») et étiquettes libres, côté app uniquement. Le hostname reste visible en second.
- **Export / import** de la configuration complète (listes, groupes, affectations, horaires, postes, noms, historique) :
  - **sans la CA** : fichier JSON de sauvegarde
  - **avec la CA** : pour migrer vers un autre Mac. Le fichier est **chiffré par phrase de passe** (crate `age`), car sans la CA la nouvelle app ne peut plus parler aux agents.
- **Désinstallation à distance** : restaure le hosts et les politiques navigateurs, supprime le service et les fichiers, puis révoque le cert côté app.

## 7. Modèle de politique et API agent

### Politique envoyée à l'agent (déjà résolue par l'app)
```json
{
  "version": 14,
  "timezone": "Europe/Paris",
  "rules": [
    { "id": "social", "name": "Réseaux sociaux", "domains": ["tiktok.com", "www.tiktok.com", "..."], "schedule": null },
    { "id": "games", "name": "Jeux", "domains": ["roblox.com", "..."],
      "schedule": [ { "days": ["mon","tue","wed","thu","fri"], "from": "08:00", "to": "18:00" } ] }
  ],
  "exceptions": [ { "domains": ["twitch.tv", "www.twitch.tv"], "until": "2026-09-23T20:15:00+02:00" } ],
  "protection": { "self_heal": true, "disable_doh": true }
}
```
Domaines bloqués à l'instant T = ∪ (règles actives à T) − (exceptions non expirées).

### API (mTLS, port 7878)

| Méthode | Route                 | Rôle                                                                  |
|---------|-----------------------|-----------------------------------------------------------------------|
| GET     | `/v1/info`            | hostname, OS, version agent, uptime                                   |
| GET     | `/v1/status`          | version de politique appliquée, domaines effectifs, hash du bloc, intégrité, état DoH, prochaine transition |
| PUT     | `/v1/policy`          | remplace la politique (`If-Match: <version>`, sinon 409)              |
| GET     | `/v1/events?since=N`  | événements du journal local depuis la séquence N                      |
| POST    | `/v1/update`          | nouveau binaire agent (flux + SHA-256)                                |
| POST    | `/v1/uninstall`       | restaure hosts + politiques navigateurs, supprime service et certs    |

### Écriture du fichier hosts
- L'agent ne touche **que** le bloc `# BEGIN HOMEGUARD` … `# END HOMEGUARD` (+ lignes conflictuelles commentées, cf. 6.3). Le reste du fichier est préservé.
- Chaque domaine est écrit sous la forme `0.0.0.0 domaine`.
- Écriture atomique (fichier temp + rename) et sauvegarde `hosts.homeguard.bak` à l'installation.
- Flush DNS : `ipconfig /flushdns` sous Windows ; `resolvectl flush-caches` sous Linux (si systemd-resolved). Les navigateurs gardent leur propre cache environ 1 min.
- Chemins : `C:\Windows\System32\drivers\etc\hosts` et `/etc/hosts`. Préserver CRLF ou LF selon le fichier.
- Note : Defender peut signaler un hosts qui bloque des domaines Microsoft. Pas d'impact pour les autres domaines.

## 8. Mockups

Navigation latérale : Tableau de bord · Postes · Groupes · Listes · Historique · Réglages.
Légende : ● en ligne · ◐ écart · ○ hors ligne.

### Tableau de bord (écran d'accueil)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●                          HomeGuard                         ⟳  ＋ Postes │
├──────────────────┬───────────────────────────────────────────────────────────┤
│ ▸ Tableau de bord│  Tableau de bord                     synchronisé à 19:42  │
│   Postes       4 │                                                           │
│   Groupes      2 │  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐  │
│   Listes       3 │  │ En ligne  │ │ En écart  │ │Altérations│ │ Agents à  │  │
│   Historique     │  │   3 / 4   │ │     1     │ │ 2 · 24 h  │ │ mettre à  │  │
│   Réglages       │  │           │ │           │ │           │ │  jour : 1 │  │
│                  │  └───────────┘ └───────────┘ └───────────┘ └───────────┘  │
│                  │                                                           │
│                  │  À TRAITER                                                │
│                  │   ◐ ubuntu-bureau  politique v12 appliquée, v14 attendue  │
│                  │                                        [ Synchroniser ]   │
│                  │   ! PC-SALON       hosts modifié à 16:02 → corrigé        │
│                  │   ↑ PC-CHAMBRE     agent 0.1.0 (0.2.0 dispo)  [ MàJ ]     │
│                  │                                                           │
│                  │  DÉBLOCAGES ACTIFS                                        │
│                  │   twitch.tv · PC de Léo · jusqu'à 20:15      [ Révoquer ] │
│                  │                                                           │
│                  │  PROCHAINES TRANSITIONS                                   │
│                  │   18:00  « Jeux » débloquée      · groupe Enfants (2)     │
│                  │   21:00  « Streaming » bloquée   · PC-SALON               │
├──────────────────┴───────────────────────────────────────────────────────────┤
│ CA a3:4f:…:9c · 3/4 postes en ligne · auto-sync activée                      │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Détail d'un poste

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●                          HomeGuard                         ⟳  ＋ Postes │
├──────────────────┬───────────────────────────────────────────────────────────┤
│   Tableau de bord│  PC de Léo  ✎                                       ⋯     │
│ ▸ Postes       4 │  PORTABLE-LEO · Windows 11 · 192.168.1.34 · agent 0.2.0   │
│   Groupes      2 │  ● En ligne · politique v14 appliquée · intègre           │
│   Listes       3 │  Groupes : [ Enfants × ] [ ＋ ]   Étiquettes : [ ado × ]  │
│   Historique     │ ───────────────────────────────────────────────────────── │
│   Réglages       │  BLOCAGES                                  [ ＋ Liste ]   │
│                  │   Liste              Via        Horaire          Actif    │
│ ● PC de Léo      │   Réseaux sociaux    Enfants    toujours         ● oui    │
│ ● PC-SALON       │   Jeux               Enfants    L–V 08:00–18:00  ● oui    │
│ ◐ ubuntu-bureau  │   Streaming          ce poste   L–D 21:00–07:00  ○ non    │
│ ○ PC-CHAMBRE     │                                                           │
│   hors ligne 2 j │  DÉBLOCAGES TEMPORAIRES                 [ Débloquer… ]    │
│                  │   twitch.tv          jusqu'à 20:15 (reste 28 min)    ✕    │
│                  │                                                           │
│                  │  PROTECTION                                               │
│                  │   ☑ Auto-réparation du hosts   ☑ Bloquer DNS-over-HTTPS   │
│                  │   ! 1 altération corrigée aujourd'hui à 16:02  → voir     │
│                  │                                                           │
│                  │  Domaines bloqués maintenant : 38        [ Voir la liste ]│
│                  │                                                           │
│                  │  ● 1 modification non appliquée   [ Annuler ] [Appliquer] │
├──────────────────┴───────────────────────────────────────────────────────────┤
│ CA a3:4f:…:9c · 3/4 postes en ligne · auto-sync activée                      │
└──────────────────────────────────────────────────────────────────────────────┘
```

Menu ⋯ : Renommer · Mettre à jour l'agent · Voir l'historique · Désinstaller à distance.

### Listes

```
┌──────────────────┬───────────────────────────────────────────────────────────┐
│   Tableau de bord│  LISTES                   │  Jeux  ✎                      │
│   Postes       4 │                           │  Utilisée par : Enfants       │
│   Groupes      2 │  Réseaux sociaux     12   │   (L–V 08–18), PC-SALON       │
│ ▸ Listes       3 │ ▸Jeux                 7   │ ───────────────────────────── │
│   Historique     │  Streaming            5   │  roblox.com          +2   ✕   │
│   Réglages       │                           │  fortnite.com        +1   ✕   │
│                  │                           │  epicgames.com       +1   ✕   │
│                  │                           │  minecraft.net       +1   ✕   │
│                  │                           │  …                            │
│                  │  [ ＋ Nouvelle liste ]    │  [ ajouter / coller… ]  [＋]  │
│                  │                           │  +N = variantes auto (www.,   │
│                  │                           │  m., alias) · clic pour voir  │
└──────────────────┴───────────────────────────────────────────────────────────┘
```

### Affecter une liste avec un horaire

```
┌──────────────── Bloquer « Jeux » pour le groupe Enfants ─────────────────┐
│                                                                          │
│  Quand ?   ( ) Toujours    (•) Selon un horaire                          │
│                                                                          │
│         0     3     6     9     12    15    18    21    24               │
│  Lun    ░░░░░░░░░░░░░░░░████████████████████░░░░░░░░░░░░                 │
│  Mar    ░░░░░░░░░░░░░░░░████████████████████░░░░░░░░░░░░                 │
│  Mer    ░░░░░░░░░░░░░░░░████████████████████░░░░░░░░░░░░                 │
│  Jeu    ░░░░░░░░░░░░░░░░████████████████████░░░░░░░░░░░░                 │
│  Ven    ░░░░░░░░░░░░░░░░████████████████████░░░░░░░░░░░░                 │
│  Sam    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░                 │
│  Dim    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░                 │
│         ██ bloqué   ░░ autorisé   (glisser pour peindre, pas 30 min)     │
│                                                                          │
│  Fuseau : Europe/Paris (fixe, indépendant du poste)                      │
│                                                                          │
│                                          [ Annuler ]  [ Enregistrer ]    │
└──────────────────────────────────────────────────────────────────────────┘
```

### Déblocage temporaire

```
┌────────────── Débloquer temporairement · PC de Léo ──────────────┐
│                                                                  │
│  Quoi ?    (•) Domaine  [ twitch.tv                ]             │
│            ( ) Liste    [ Jeux                   ▾ ]             │
│                                                                  │
│  Durée ?   [15 min] [30 min] [■ 1 h ■] [2 h] [Jusqu'à ce soir]   │
│                                                                  │
│  Rebloqué automatiquement à 20:42, même si l'app est fermée.     │
│                                                                  │
│                                   [ Annuler ]  [ Débloquer ]     │
└──────────────────────────────────────────────────────────────────┘
```

### Historique

```
┌──────────────────┬───────────────────────────────────────────────────────────┐
│   Tableau de bord│  HISTORIQUE      [Tous postes ▾] [Tous types ▾] [7 j ▾] ⤓ │
│   Postes       4 │                                                           │
│   Groupes      2 │  AUJOURD'HUI                                              │
│   Listes       3 │  19:42  app    PC de Léo      Politique v14 appliquée     │
│ ▸ Historique     │  19:41  app    PC de Léo      Déblocage twitch.tv → 20:15 │
│   Réglages       │  18:00  agent  PC de Léo      Horaire : « Jeux » débloquée│
│                  │  16:02  agent  PC-SALON     ! hosts modifié → corrigé     │
│                  │  09:13  agent  PC-SALON     ! Service arrêté manuellement │
│                  │  09:15  agent  PC-SALON       Service redémarré           │
│                  │  08:00  agent  PC de Léo      Horaire : « Jeux » bloquée  │
│                  │  HIER                                                     │
│                  │  21:10  app    PC-SALON       Agent mis à jour 0.1 → 0.2  │
│                  │  20:55  app    Enfants        Liste « Jeux » affectée     │
└──────────────────┴───────────────────────────────────────────────────────────┘
```

### Ajouter des postes

```
┌────────────────────────── Ajouter des postes ───────────────────────────┐
│                                                                         │
│  Enrôlement actif · expire dans 28:41 · 1 poste enrôlé                  │
│  Groupe par défaut : [ Enfants ▾ ]                                      │
│                                                                         │
│  [ Windows ]  Linux   Hors-ligne                                        │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │ irm http://192.168.1.20:7880/install.ps1?t=K7Q2-9XPA | iex        │📋│
│  └───────────────────────────────────────────────────────────────────┘  │
│  À exécuter dans PowerShell en administrateur sur le poste.             │
│                                                                         │
│  Postes enrôlés pendant cette session :                                 │
│   ✓ PORTABLE-LEO   Windows 11   19:40                                   │
│   … en attente                                                          │
│                                                                         │
│                                  [ Nouveau jeton ]   [ Terminer ]       │
└─────────────────────────────────────────────────────────────────────────┘
```

### Premier lancement

```
┌──────────────────────── Bienvenue ─────────────────────────┐
│                                                            │
│  HomeGuard va créer une autorité de certification          │
│  privée pour sécuriser la communication avec vos postes.   │
│                                                            │
│  Les clés seront stockées dans votre Trousseau macOS.      │
│                                                            │
│  Vous avez déjà une sauvegarde ?  [ Importer… ]            │
│                                                            │
│                         [ Créer et continuer ]             │
└────────────────────────────────────────────────────────────┘
```

## 9. Stockage côté app (SQLite, `~/Library/Application Support/homeguard/`)

| Table            | Contenu                                                                        |
|------------------|--------------------------------------------------------------------------------|
| `agents`         | id, hostname, nom affiché, étiquettes, OS, dernière IP, serial cert, révoqué, vu le, version agent, dernier status |
| `groups`, `group_members` | groupes et appartenance (N–N)                                         |
| `lists`, `list_domains`   | listes, domaines, variantes exclues |
| `aliases`        | catalogue de variantes (initialisé depuis `aliases.toml`)                      |
| `assignments`    | liste → cible (groupe ou poste), horaire optionnel                             |
| `exceptions`     | déblocages temporaires (cible, domaines ou liste, échéance)                    |
| `policies`       | par poste : version courante attendue + contenu + hash                         |
| `history`        | horodatage, source (app/agent), poste/groupe, type, détails JSON              |
| `agent_cursors`  | dernière séquence d'événements lue par agent                                   |
| `settings`       | fuseau par défaut, auto-sync, seuils d'alerte                                  |

## 10. Crates principales

| Rôle                     | Crate                                        |
|--------------------------|----------------------------------------------|
| HTTP serveur             | `axum`, `axum-server` (rustls)               |
| HTTP client              | `reqwest` (rustls, identité client)          |
| TLS / PKI                | `rustls`, `rcgen`, `x509-parser`             |
| mDNS                     | `mdns-sd`                                    |
| Trousseau macOS          | `security-framework`                         |
| Service Windows          | `windows-service`                            |
| Registre Windows         | `winreg`                                     |
| Surveillance fichiers    | `notify`                                     |
| Dates / fuseaux          | `jiff` (tzdb embarquée)                      |
| Auto-mise à jour         | `self-replace`                               |
| Chiffrement export       | `age`                                        |
| Hash                     | `sha2`                                       |
| CLI agent                | `clap`                                       |
| Stockage app             | `rusqlite`                                   |
| Sérialisation            | `serde`, `serde_json`, `toml`                |

## 11. Étapes

Légende : ✅ fait et testé (Linux, ce sandbox) · 🟡 fait mais non vérifiable ici (nécessite Windows ou macOS) · ⬜ pas commencé.

1. ✅ **common** : modèle de politique, évaluation des horaires (tests à heure fixe, changements d'heure été/hiver), parsing/écriture du bloc hosts (CRLF, bloc absent, bloc corrompu, lignes conflictuelles). Plus : PKI (CA, CSR, signature), configuration mTLS, catalogue d'alias. 30 tests.
2. ✅🟡 **agent — cœur** : API + mTLS avec certs de test, évaluateur et timer, écriture hosts, journal d'événements, auto-réparation, anti-DoH, mise à jour à distance. 20 tests (dont 6 de bout en bout, un vrai handshake mTLS). Le service Windows (`service.rs`, `windows-service`) est maintenant complet — enregistrement SCM **et** dispatcher (`define_windows_service!` + gestionnaire de contrôle qui distingue `STOP` de `SHUTDOWN`/`PRESHUTDOWN`, repli automatique en mode premier plan quand le binaire n'est pas lancé par le SCM) — compile réellement en cross-compilation (`x86_64-pc-windows-gnu`, `cargo clippy` inclus, zéro avertissement) mais reste non exécutable ici : à valider sur une VM Windows. La partie Linux (systemd) est réelle et testée.
3. ✅ **PKI + enrôlement** : génération CA/client (`common::pki`), endpoint `/v1/enroll` (`app-core::enrollment_server`), CLI `enroll`/`install`/`uninstall` côté agent. 12 tests, dont l'enrôlement complet via un vrai handshake TLS avec épinglage de la CA par empreinte.
4. ✅ **app — cœur + shell Tauri** : `app-core` (base SQLite, moteur de résolution de politique, client mTLS, découverte mDNS, serveur d'enrôlement) est fait et testé. Le **shell Tauri** (`app/`) existe et a été validé **directement sur le Mac** : `cargo tauri dev` compile et lance une vraie fenêtre, l'interface répond. Les trois points spécifiques à macOS sont faits et vérifiés :
   - **CA dans le Trousseau** (`keychain_ca_store.rs`, via `security-framework`) : migration testée depuis l'ancien stockage fichier, même empreinte SHA-256 conservée, clés jamais sur disque en clair.
   - **`Info.plist` mDNS** (`NSLocalNetworkUsageDescription` + `NSBonjourServices`) : confirmé dans le vrai bundle généré (`plutil -p`), et la demande d'autorisation macOS apparaît réellement à l'usage.
   - **Icônes** : icône « bouclier » générée et intégrée (`.icns`/`.ico` via `tauri icon`), en attente d'une identité visuelle définitive si tu en veux une plus tard.
   - Au passage : l'identifiant de bundle a été corrigé (`com.homeguard.app` → `com.homeguard.desktop`, l'ancien terminait par `.app` ce que Tauri déconseille), et `tauri build --debug` produit un `.app` + `.dmg` fonctionnels.
   - **Passe de design** : mode sombre (suit macOS automatiquement), système de tokens (couleur/espacement/typo), pastilles de statut au lieu de texte brut, chips pour les listes de domaines au lieu d'un tableau à une colonne, icônes SVG maison dans la barre latérale et le tableau de bord, filtre dans l'historique, états vides avec icône. Validé à l'écran par toi sur le Mac.
5. ✅ **Déploiement** : le serveur de déploiement (`app-core::deploy_server`) est fait et testé — il sert `install.sh`/`install.ps1` avec les placeholders substitués (URL d'enrôlement, jeton, empreinte de CA, URLs des binaires) et sert les binaires agent embarqués. Câblé dans le dialogue « Ajouter des postes », qui démarre maintenant le serveur d'enrôlement HTTPS (port 7879) **et** ce serveur HTTP (port 7880) ensemble. Les trois binaires agent (Linux x86_64/arm64 statiques via `cargo-zigbuild`, Windows x86_64 GNU via `mingw-w64`) sont cross-compilés depuis le Mac par `scripts/build-agents.sh` et déposés dans `app/src-tauri/resources/agents/` (ignoré par git, à reconstruire localement) ; vérifié bout en bout que `tauri build` les embarque au bon endroit dans `Contents/Resources/agents/`, donc `binaries_available: true` et l'installateur en une commande sert vraiment un binaire.
6. ✅ **Planification** : grille horaire et déblocages temporaires évalués dans `common::policy` (testé, y compris le changement d'heure été/hiver et les horaires à cheval sur minuit). « Prochaines transitions » affiché sur le tableau de bord.
7. ✅🟡 **Intégrité** : `/v1/status`, détection d'écart et auto-réparation testés de bout en bout. Anti-DoH : Linux réel et testé (Chrome/Edge/Brave/Chromium + fusion Firefox), Windows écrit mais non vérifiable ici (registre). **Détection d'un arrêt manuel** : faite et testée sur Linux — `shutdown.rs` distingue Ctrl+C/`systemctl stop`/`kill` (manuel, journalisé) d'une extinction du poste (silencieux, via l'heuristique `systemctl is-system-running`), validée par un test qui envoie un vrai SIGTERM au process de test. Windows : la distinction est maintenant câblée bout en bout via le dispatcher de service (point 2) — le gestionnaire de contrôle SCM transmet directement `STOP`→manuel / `SHUTDOWN`,`PRESHUTDOWN`→extinction, sans heuristique — mais reste à valider sur une vraie VM Windows.
8. ✅ **Visibilité** : historique unifié — `sync_agent` récupère désormais `/v1/events` à chaque synchronisation et les fusionne dans l'historique de l'app (idempotent, horodatage d'origine préservé). Tableau de bord : écarts détectés (intégrité avant correction, pas après) et prochaines transitions (calculées dans `common::policy::TimeWindow::next_transition`, testé y compris weekend et horaires de nuit).
9. ✅ **Variantes** : catalogue d'alias + expansion (`common::aliases`), testé.
10. ✅🟡 **Maintenance** : désinstallation à distance faite et testée. **Mise à jour à distance** : bout en bout depuis l'écran Postes — un poste détecté obsolète (comparaison simple de chaîne entre `agent.agent_version` et `resources/agents/version.txt`, pas de vraie sémantique semver) affiche un bouton « Mettre à jour » qui appelle la nouvelle commande `update_agent` ; celle-ci choisit le bon binaire embarqué via `(os, arch)` (`deploy_server::AgentTarget::from_os_arch`), calcule son SHA-256 (`homeguard_common::sha256_hex`, factorisé — utilisé aussi par `deploy_server`) et l'envoie par `AgentClient::push_update` → `POST /v1/update` côté agent, qui vérifie le checksum et se remplace lui-même (`self-replace`) avant de redémarrer sous le service manager. Le mécanisme bas niveau est testé en isolation (agent et app-core) ; la commande Tauri elle-même ne l'est pas (comme le reste de `commands.rs`, jamais testé directement — glue non couverte). **Vérification post-mise à jour** : `update_agent` ne s'arrête plus à l'acceptation de la requête — elle attend le délai de redémarrage du service manager (`RestartSec=5`/`restart/5000`, plus la marge de `post_update`), puis reconnecte et lit `/v1/info` jusqu'à confirmation (jusqu'à ~17s, plusieurs tentatives) ; le résultat (`UpdateOutcome`) distingue poste revenu avec la bonne version, revenu avec une version inattendue, ou aucune réponse. La base est mise à jour avec la version confirmée dès qu'elle est connue, donc le badge « obsolète » disparaît sans attendre une synchronisation complète. Le bouton « Mettre à jour » de l'écran Postes affiche le résultat détaillé. **Action groupée** : le tableau de bord a une carte « Mises à jour » listant les postes obsolètes et un bouton « Mettre à jour tout » (`update_all_outdated_agents`, séquentiel comme `discover_and_sync_all` — un poste qui ne répond jamais ne bloque que sa propre entrée, pas les autres) ; la logique d'envoi+vérification est partagée avec le bouton individuel via `perform_update`, pas dupliquée. Noms/étiquettes de poste : reliés à l'UI (écran Postes). **Statut réel en ligne/hors ligne** : l'écran Postes sonde chaque poste connu (`/v1/status`, sans effet de bord) à l'ouverture plutôt que de se fier à la simple présence d'une IP enregistrée. **Export/import chiffré** : fait et testé (`app-core::export`, chiffrement `age` par phrase de passe) — sauvegarde tout (postes, groupes, listes, affectations, exceptions, réglages) **et** la CA, ce qui permet une vraie migration vers un autre Mac sans réenrôler les postes ; import idempotent (ré-importer ne duplique rien), écran « Réglages » dans l'UI.
11. ✅🟡 **Compilation croisée des agents** : `scripts/build-agents.sh` (documenté, prérequis en commentaire) compile les 3 cibles (`cargo-zigbuild` pour Linux musl x86_64/arm64, toolchain `mingw-w64` pour Windows GNU) et les dépose dans `app/src-tauri/resources/agents/`, embarquées par Tauri comme ressources du bundle — vérifié par un vrai `tauri build --debug`. Reste ⬜ : signature/notarisation macOS, CI qui rejoue ce script automatiquement.

## 12. Points ouverts

- Postes sur un autre sous-réseau que le Mac ?
- Fuseau unique pour tout le parc ou réglable par poste ? Par défaut, fuseau global avec surcharge possible par poste.
- Firefox en snap (Ubuntu) : chemin de politiques différent, à valider sur les distributions cibles.
- Le fichier hosts ne supporte pas les jokers (`*.domaine`). Les variantes connues passent par le catalogue d'alias, les autres sous-domaines ne sont pas couverts.
