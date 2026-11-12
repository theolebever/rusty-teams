<div align="center">

<img src="crates/app/assets/icon/teams-fast-256.png" alt="Logo Rusty Teams" width="112">

# Rusty Teams · fork Rune

Client Microsoft Teams natif en Rust. Ce fork développe Rune comme interface principale et conserve le client GPUI du projet original.

[![Rust](https://img.shields.io/badge/Rust-2024-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tests](https://github.com/theolebever/rusty-teams/actions/workflows/test.yml/badge.svg)](https://github.com/theolebever/rusty-teams/actions/workflows/test.yml)
[![Build](https://github.com/theolebever/rusty-teams/actions/workflows/build.yml/badge.svg)](https://github.com/theolebever/rusty-teams/actions/workflows/build.yml)
[![Licence MIT](https://img.shields.io/badge/licence-MIT-2ea043)](LICENSE)

[État du fork](#état-du-fork) · [Démarrer](#démarrer) · [Architecture](#architecture) · [Projet original](https://github.com/danieldaeschle/rusty-teams)

</div>

Projet non officiel, sans affiliation avec Microsoft. La connexion utilise Teams sur le web, sans enregistrement d’application ni consentement administrateur supplémentaire. Les API utilisées restent soumises aux autorisations du compte et aux politiques du tenant.

## État du fork

Ce dépôt est le fork [theolebever/rusty-teams](https://github.com/theolebever/rusty-teams) du projet de [Daniel Daeschle](https://github.com/danieldaeschle/rusty-teams). Le client original fournit la base : synchronisation Teams, stockage local, appels natifs et interface GPUI.

La direction de ce fork est de porter l’interface sur [Rune](https://github.com/anomalyco/rune), en partageant le backend avec GPUI. Rune est l’interface principale du développement du fork ; son portage reste en cours et la parité fonctionnelle avec GPUI est incomplète.

| Interface | Code et exécutable | Situation actuelle |
|---|---|---|
| Rune | `crates/app-rune`, `rusty-teams-rune` | Interface principale du fork, construite par la CI. Connectée au backend Teams réel, avec un mode démo indépendant du compte. |
| GPUI | `crates/app`, `teams` | Interface historique conservée et enrichie par les intégrations du projet original. Compilation depuis les sources. |

Dernière intégration amont documentée : [`53a311d`](https://github.com/danieldaeschle/rusty-teams/commit/53a311dd86ad10c6cb8c1b9e3194549a8d55a58f), fusionnée dans `main` par [`ac43e65`](https://github.com/theolebever/rusty-teams/commit/ac43e65df66d251db6758857e9bfb5c70d5141ed) le 12 novembre 2026. Les commits amont suivants doivent être examinés lors des prochaines intégrations. [Comparer les branches actuelles](https://github.com/danieldaeschle/rusty-teams/compare/main...theolebever:rusty-teams:main).

### Différences avec le projet original

- Une interface Rune pour les conversations, les canaux, le calendrier et les appels.
- Un crate `app-backend` commun aux deux interfaces, sans dépendance à GPUI, qui regroupe la connexion, le lancement des appels, les notifications et les préférences partagées.
- Des adaptations de connexion : option `--no-sso` pour choisir un autre compte Windows et User-Agent mobile appliqué à la page Teams avant sa première navigation.
- Des optimisations du chargement des conversations, des avatars et des réactions. Rune précharge les conversations récentes sans cache et réduit les aperçus d’images hors du thread d’interface.
- Une distribution Windows Rune avec exécutable portable à chaque build et installateur par utilisateur sur les tags. Le choix manuel du compte est le comportement par défaut.

Les transports HTTP et Trouter natifs, la mise en veille des WebViews et de nombreux correctifs d’appels sont désormais partagés avec l’amont. Leur présence dans ce dépôt ne signifie pas qu’ils sont propres au fork.

### Ce qui est présent dans Rune

Le portage comprend les fonctions suivantes ; leur présence dans le code ne garantit pas encore tous les comportements du client GPUI :

- Liste des conversations, équipes et canaux, historique paginé, fils de discussion, recherche locale et navigation rapide.
- Composition de messages avec mise en forme, mentions, réactions, GIFs et pièces jointes. Le sélecteur OneDrive propose les fichiers récents, les dossiers et les fichiers partagés.
- Menus contextuels, indicateurs de saisie, messages animés, images, fichiers et lecture des messages vocaux.
- Calendrier, agenda, présence, profils et notifications natives.
- Appels audio et vidéo sur le moteur WebRTC commun, appels entrants, partage d’écran et commandes d’appel.
- Thèmes partagés, suivi du thème système et panneau d’exécution d’agents CLI.

GPUI conserve une interface de réglages plus complète et l’intégration du serveur MCP avec ses permissions et ses propositions à valider. Les réglages Rune couvrent actuellement l’apparence ; le panneau d’agents CLI de Rune ne remplace pas cette intégration MCP.

Windows est la plateforme de travail principale du fork. Les parcours Rune sous Linux et macOS restent à valider. Le client GPUI conserve les chemins de connexion et les scripts de compilation multiplateformes du projet original.

## Démarrer

Les commandes suivantes se lancent à la racine du dépôt avec un outil Rust récent compatible avec l’édition 2024.

```bash
git clone https://github.com/theolebever/rusty-teams.git
cd rusty-teams
```

### Rune, interface principale

Pour essayer l’interface avec des données de démonstration, sans compte :

```bash
cargo run -p app-rune -- --demo
```

Pour se connecter à Teams sous Windows en choisissant le compte :

```bash
cargo run -p app-rune
```

Rune désactive le SSO Windows par défaut. `--no-sso` reste accepté ; `--sso` réactive explicitement la connexion avec le compte Windows. La connexion interactive demande le runtime WebView2.

| Usage | Commande |
|---|---|
| Compiler Rune | `cargo build -p app-rune --release` |
| Consulter les options | `cargo run -p app-rune -- --help` |
| Désactiver les actions sur le compte depuis l’interface | `cargo run -p app-rune -- --read-only` |
| Utiliser un cache et un profil WebView2 séparés | `cargo run -p app-rune -- --database ./data-rune/cache.sqlite3 --no-sso` |
| Se connecter à un navigateur existant via CDP | `cargo run -p app-rune -- --endpoint http://127.0.0.1:9222` |

Le profil WebView2 se trouve à côté de la base choisie avec `--database`. Pour le mode CDP, le navigateur doit être démarré avec `--remote-debugging-port=9222` et connecté à Teams ; certains tenants nécessitent aussi un onglet Outlook pour les autorisations de messagerie.

### GPUI, interface historique

```bash
cargo run -p app -- --demo
```

Pour une connexion réelle sous Windows avec choix du compte :

```bash
cargo run -p app -- --no-sso
```

| Usage | Commande |
|---|---|
| Compiler GPUI | `cargo build -p app --release` |
| Connexion via CDP | `cargo run -p app -- --endpoint http://127.0.0.1:9222` |
| Compilation Windows depuis WSL/Linux | `scripts/build-windows.sh crates/app teams` |
| Application macOS universelle, avec les outils Xcode CLT | `scripts/build-macos.sh crates/app teams` |
| Capture GPUI sous Linux | `cargo build -p app && scripts/headless-shot.sh shot.png` |
| Vérification des dispositions GPUI | `cargo build -p app && scripts/layout-sweep.sh` |

Sous Linux, la compilation GPUI nécessite notamment clang 21 ou plus récent pour libwebrtc et mold, configuré dans `.cargo/config.toml`. Voir [apt.llvm.org](https://apt.llvm.org) si la version de clang fournie par la distribution est trop ancienne. Les appels nécessitent aussi glib à l’exécution. Sous Windows, les builds natifs utilisent les outils C++ de Visual Studio et le SDK Windows ; la préparation complète des artefacts est décrite dans [l’action de compilation Windows](.github/actions/setup-windows/action.yml).

### Binaires et CI

Le workflow [Build de ce fork](https://github.com/theolebever/rusty-teams/actions/workflows/build.yml) construit Rune. Chaque build produit le portable Windows `rusty-teams-rune.exe`, le binaire Linux et une application macOS universelle. Le portable Windows est téléchargeable dans l’artefact `teams-windows-amd64-portable`. L’installateur Windows est construit et envoyé comme artefact uniquement sur un tag.

| Workflow | Déclenchement | Contenu |
|---|---|---|
| [Test](.github/workflows/test.yml) | Push sur `main` ou lancement manuel | Tests du workspace, dont Rune, sous Linux et Windows. |
| [Build](.github/workflows/build.yml) | Push sur `main`, push d’un tag ou lancement manuel | Compilation et empaquetage Rune sous Linux, Windows et macOS. Portable Windows à chaque build, installateur sur les tags, publication d’une release pour les tags commençant par `v`. |

Le mécanisme de recherche de mises à jour GPUI référence encore les releases du dépôt original. Il reste à adapter à la distribution de ce fork.

## Architecture

Les deux interfaces utilisent le même stockage et les mêmes services Teams. Le backend partagé permet de faire évoluer Rune tout en conservant le client GPUI.

| Crate | Rôle |
|---|---|
| `app-rune` | Interface Rune et binaire `rusty-teams-rune`. |
| `app` | Interface GPUI et binaire `teams`. |
| `app-backend` | Runtime, connexion, lancement des appels, notifications et préférences communs. |
| `app-state`, `app-call-model`, `app-ui-kit`, `app-base` | État, modèles et composants utilisés par GPUI. |
| `core` | Synchronisation, actions Teams et transformation des données. |
| `store` | Cache SQLite, migrations et recherche locale FTS5. |
| `graph`, `chatsvc` | Clients Microsoft Graph et services Teams : messages, canaux, présence et événements temps réel. |
| `calling` | Signalisation Teams, traduction SDP et audio/vidéo natifs via libwebrtc. |
| `session` | Sessions, acquisition des jetons et transport HTTP natif, avec repli sur les requêtes dans la page. |
| `webview`, `browser` | Hôte WebView2 Windows ou gestion d’un Chrome accessible via CDP. |
| `agents`, `mcp` | Exécution d’agents CLI et serveur MCP. Leur intégration dépend de l’interface. |
| `cli` | Outil de diagnostic `teams-probe`. |

L’authentification passe par une vraie page Teams. Rust obtient les jetons d’accès depuis cette page et les conserve en mémoire pour envoyer les requêtes HTTP natives. Le code Rust ne les écrit pas sur disque et ne les journalise pas ; le navigateur gère son propre profil et son cache d’authentification.

Le WebSocket Trouter peut également fonctionner depuis Rust. Les échanges qui nécessitent les cookies de la page continuent de passer par le navigateur. `TEAMS_PAGE_FETCH=1` force les requêtes HTTP à repasser par la page. Les WebViews peuvent être mises en veille lorsqu’elles ne sont plus nécessaires.

## Validation et limites

```bash
cargo test --workspace --no-fail-fast
```

La validation locale Windows de la fusion `ac43e65` a couvert 3 772 tests, avec 2 tests ignorés. La suite HTTP MCP a dû être relancée en séquentiel : le test de rejet d’une requête surdimensionnée échouait en parallèle avec une connexion interrompue sous Windows. Les 39 tests de cette suite passent avec :

```bash
cargo test -p mcp --test http -- --test-threads=1
```

Ces résultats concernent les tests automatisés locaux. Ils ne constituent pas une validation complète des parcours sur un compte Teams réel ni des trois plateformes. Les badges en tête de page donnent accès à l’état de la CI du fork.

Les budgets d’images de Rune limitent la rétention des images de messages avec un délai de grâce pour les images récemment affichées. Ils ne constituent pas une limite stricte de la mémoire totale : les avatars et certains coûts d’animation restent en dehors de ce budget.

Le fichier [ROADMAP.md](ROADMAP.md) conserve la feuille de route héritée du projet original. Pour le fork, les écarts actuels comprennent la parité Rune/GPUI et la validation des binaires Rune sur chaque plateforme.

## Crédits et licence

Le projet original et l’essentiel du client Teams sont développés dans [danieldaeschle/rusty-teams](https://github.com/danieldaeschle/rusty-teams). Les modifications de ce fork sont suivies dans [theolebever/rusty-teams](https://github.com/theolebever/rusty-teams).

[Licence MIT](LICENSE).
