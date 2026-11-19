# CuprIDE

**CuprIDE** est un IDE multiplateforme écrit en Rust, conçu pour être **le plus léger et le plus rapide possible**, et **entièrement personnalisable**.

Il suit la philosophie d'Arch Linux : un noyau minimal, puis chacun installe uniquement les outils dont il a besoin, via un gestionnaire de paquets intégré. L'IDE est **piloté par des commandes** : chaque action est une commande, accessible depuis la ligne de commande interne, un raccourci, la palette ou le terminal du système.

> **État du projet : phase 0 (mise en place).** Le code ne contient pas encore d'éditeur fonctionnel. Voir la [feuille de route](#feuille-de-route).

## Objectifs

- Éditer du code dans tous les langages courants (C, C++, Java, C#, Python, Rust, Go…) grâce à **Tree-sitter**, **LSP** et **DAP**.
- Composer librement son interface : splits, onglets, panneaux, layouts nommés.
- Plusieurs fenêtres sur plusieurs écrans, avec **une seule instance** de l'application.
- Étendre l'IDE par des **plugins WebAssembly** isolés, installables depuis un registre.
- Tout configurer par fichiers texte (TOML) : raccourcis, layouts, thèmes.
- Fonctionner sur **Windows, macOS et Linux** (X11 et Wayland).

### Objectifs de performance

| Mesure | Cible |
|---|---|
| Démarrage | < 100 ms |
| Mémoire au repos | < 50 Mo |
| CPU au repos | 0 % |
| Latence de frappe | imperceptible |
| Gros fichiers | plusieurs centaines de Mo, sans ralentissement |

Hors périmètre : aucun moteur web embarqué (ni Electron, ni webview), aucun langage pris en charge en dur dans le noyau.

## Stack technique

| Domaine | Choix |
|---|---|
| Fenêtres et événements | `winit` |
| Rendu GPU | `wgpu` (Vulkan, Metal, DirectX 12, OpenGL) |
| Interface | couche maison, minimale |
| Texte | `cosmic-text`, `glyphon` |
| Buffer | rope maison |
| Syntaxe | `tree-sitter` |
| Langages | client LSP générique (`lsp-types`), DAP |
| Commandes | `clap`, `shlex`, `nucleo` (recherche floue) |
| Instance unique | `interprocess` |
| Configuration | TOML (`serde`, `toml`), layouts en JSON |
| Terminal intégré | `portable-pty`, `alacritty_terminal` |
| Plugins | `wasmtime` + Component Model, interfaces WIT |
| Paquets | `pubgrub`, signatures ed25519 |

## Prérequis

### Natif

- **Rust** stable, édition 2024 (version alignée sur l'image Docker : 1.95). Installation via [rustup](https://rustup.rs).
- Composants : `rustup component add rustfmt clippy`
- Optionnel : `cargo install cargo-deny --locked`

Bibliothèques système selon l'OS :

- **Linux (Debian / Ubuntu)** :
  ```sh
  sudo apt install pkg-config build-essential git \
      libxkbcommon-dev libwayland-dev \
      libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
      libfontconfig1-dev
  ```
- **macOS** : les outils en ligne de commande Xcode (`xcode-select --install`).
- **Windows** : les *Build Tools* de Visual Studio (composant « Développement Desktop en C++ »).

Cette liste sera complétée au fil des crates ajoutées.

### Docker (optionnel)

Docker et Docker Compose suffisent pour compiler et tester sans rien installer d'autre. CuprIDE reste une application de bureau : les images servent à compiler, vérifier et tester, pas à distribuer l'IDE.

## Démarrage rapide

```sh
git clone https://github.com/Zolkn-Sama/cupride.git
cd cupride
cargo run                    # lance l'IDE (binaire `cupride`)
cargo run -p cupride-repl    # lance le REPL console du noyau
```

Au premier appel, `rustup` installe automatiquement la version de Rust figée dans `rust-toolchain.toml`.

### Compilation

```sh
cargo build --workspace              # toutes les crates, en debug
cargo build --release --bin cupride  # binaire optimisé dans target/release/cupride
```

### Vérifications

```sh
cargo fmt --all -- --check                             # formatage
cargo clippy --workspace --all-targets -- -D warnings  # analyse statique
cargo test --workspace                                 # tests
cargo deny check                                       # licences et vulnérabilités des dépendances
```

La CI GitHub Actions exécute les mêmes vérifications sur Linux, Windows et macOS à chaque push.

## Docker

| Commande | Rôle |
|---|---|
| `docker compose run --rm dev` | Shell de développement, code monté depuis l'hôte |
| `docker compose run --rm ci` | Mêmes vérifications que la CI : fmt, clippy, tests, deny |
| `docker compose --profile bench run --rm bench` | Benchmarks du buffer (criterion) |
| `docker build --target dist --output type=local,dest=dist .` | Binaire Linux de release dans `./dist` |
| `docker compose --profile gui up --build gui` | Lancer l'IDE dans un Linux vierge (Linux + X11 uniquement) |

Sous Linux, exporter `USER_UID=$(id -u)` et `USER_GID=$(id -g)` (ou les mettre dans un fichier `.env`) pour que les fichiers créés dans le dépôt vous appartiennent.

Pour le service `gui`, autoriser l'accès à l'affichage avant, et le révoquer après :

```sh
xhost +local:
CUPRIDE_PROJECT=~/code/mon-projet docker compose --profile gui up --build gui
xhost -local:
```

Les mesures de performance faites en conteneur servent à comparer entre elles ; les objectifs se mesurent en natif.

## Variables d'environnement

| Variable | Rôle |
|---|---|
| `CUPRIDE_LOG` | Niveau de journalisation (`error`, `warn`, `info`, `debug`, `trace`, ou filtre par crate comme `cupride_core=trace,warn`). Par défaut : `warn` |
| `CUPRIDE_PROJECT` | Dossier du projet à ouvrir dans le service Docker `gui` |

Les logs sont écrits dans le dossier de cache de l'OS, avec un fichier par jour (7 conservés) :

| OS | Emplacement |
|---|---|
| Linux | `~/.cache/cupride/logs/` |
| macOS | `~/Library/Caches/cupride/logs/` |
| Windows | `%LOCALAPPDATA%\cupride\cache\logs\` |

## Architecture

Le noyau fonctionne **sans interface** : l'état de l'éditeur (documents, layout, commandes, configuration) est testable avec `cargo test` et pilotable depuis un REPL console. Le code est organisé en workspace Cargo, une crate par responsabilité, avec des dépendances uniquement vers le bas :

```
                         app
     ┌──────┬──────┬──────┼──────┬──────────┬──────┐
     ui    ipc    lsp  syntax terminal plugin-host pkg
   ┌─┴──┐          │      │              │
 core render      text   text          core
 ┌─┴──┐
text layout
```

### Arborescence

| Chemin | Crate | Rôle |
|---|---|---|
| `app/` | `cupride-app` | Binaire `cupride` : boucle d'événements, fenêtres, assemblage |
| `crates/text/` | `cupride-text` | Buffer (rope), positions, Unicode, sélections, historique |
| `crates/layout/` | `cupride-layout` | Arbre de layout, panneaux, fenêtres et écrans |
| `crates/core/` | `cupride-core` | État de l'éditeur, documents, commandes, keymap, configuration |
| `tools/repl/` | `cupride-repl` | Éditeur en console pour tester le noyau |
| `docs/commands.md` | | Référence des commandes |

Les autres crates (`render`, `ui`, `syntax`, `lsp`, `terminal`, `ipc`, `plugin-api`, `plugin-host`, `pkg`) sont ajoutées à leur phase.

## Feuille de route

| Phase | Contenu | Jalon |
|---|---|---|
| 0 | Workspace, outillage, CI | |
| 1 | Buffer de texte : rope, Unicode, sélections, historique | |
| 2 | Noyau et commandes, REPL console | |
| 3 | Première fenêtre : rendu GPU, texte, curseur, saisie | |
| 4 | Ligne de commande, palette, barre d'état, raccourcis | **v0.1** — éditeur mono-fenêtre |
| 5 | Layout : splits, onglets, panneaux, sauvegarde | |
| 6 | Multi-fenêtres, multi-écran, instance unique, CLI externe | **v0.2** — architecture complète |
| 7 | Coloration syntaxique (Tree-sitter) | |
| 8 | Client LSP | **v0.3** — utilisable au quotidien |
| 9 | Terminal intégré | |
| 10 | Plugins WASM | |
| 11 | Gestionnaire de paquets et découverte | **v0.4** — IDE extensible |
| 12 | DAP (Culprite), Git, thèmes, Lua, détachement des onglets | |

## Contribuer

Les conventions (outillage, gestion des erreurs, journalisation, branches) sont décrites dans [`CONTRIBUTING.md`](CONTRIBUTING.md). Avant de pousser : `docker compose run --rm ci`, ou les commandes de [vérification](#vérifications) en natif.

## Nom

La **cuprite** est un oxyde de cuivre : dans la communauté Rust, réécrire un logiciel en Rust se dit « l'oxyder ». Le débogueur s'appelle **Culprite** (il trouve le coupable), et les thèmes portent des noms de la famille du corindon (**Padparadscha** par défaut, Rubis, Saphir, Leuco). Le nom n'utilise pas la marque Rust, conformément à la politique de la Rust Foundation.

## Licence

CuprIDE est distribué sous double licence, au choix :

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) ou <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT ([`LICENSE-MIT`](LICENSE-MIT) ou <https://opensource.org/licenses/MIT>)

Sauf mention contraire explicite, toute contribution soumise pour inclusion dans CuprIDE, au sens de la licence Apache-2.0, est placée sous cette même double licence, sans conditions supplémentaires.
