# Configuration

CuprIDE se configure avec un fichier texte TOML, versionnable avec ses dotfiles. La commande
`config open` l'ouvre dans l'IDE ; s'il n'existe pas encore, il est prérempli avec toutes les
options, commentées, avec leur valeur par défaut. `config reload` l'applique sans redémarrer.

## Emplacement

| OS | Fichier |
|---|---|
| Linux | `~/.config/cupride/config.toml` |
| macOS | `~/Library/Application Support/cupride/config.toml` |
| Windows | `%APPDATA%\cupride\config\config.toml` |

- Fichier absent : valeurs par défaut.
- Erreur de syntaxe ou de type : message avec la ligne et la colonne
  (`config.toml:3:23 : ...`), et démarrage avec les valeurs par défaut.
- Option ou section inconnue (faute de frappe) : ignorée, avec un avertissement.

## Options

### `[editor]`

| Option | Valeurs | Défaut | Effet |
|---|---|---|---|
| `line_ending` | `"auto"`, `"lf"`, `"crlf"` | `"auto"` | Fin de ligne des **nouveaux** documents (`auto` : LF). Un fichier existant garde toujours ses fins de ligne. |
| `undo_group_delay_ms` | entier ≥ 0 | `1000` | Pause, en millisecondes, au-delà de laquelle une nouvelle frappe commence un nouveau bloc d'annulation. `0` : chaque frappe s'annule séparément. |
| `primary_selection` | `true`, `false` | `false` | Sélection primaire (Linux, X11 et Wayland) : le texte sélectionné y est copié, et le clic du milieu le colle. Sans effet sous Windows et macOS. |

### `[font]`

| Option | Valeurs | Défaut | Effet |
|---|---|---|---|
| `family` | `"monospace"` ou un nom de famille installée | `"monospace"` | Police de l'éditeur. `monospace` : police à chasse fixe du système. Une famille introuvable est signalée, et la police à chasse fixe du système est utilisée. Les ligatures de la police (Fira Code, JetBrains Mono…) sont appliquées. |
| `size` | nombre, de 4 à 200 | `14.0` | Taille en pixels logiques, multipliée par l'échelle de l'écran. |
| `line_height` | nombre, de 0,8 à 4 | `1.4` | Hauteur de ligne, en multiple de la taille. |

### `[view]`

| Option | Valeurs | Défaut | Effet |
|---|---|---|---|
| `cursor_blink` | `true`, `false` | `true` | Clignotement du curseur. Il s'arrête, curseur visible, 10 secondes après la dernière saisie : au repos, l'IDE ne consomme pas de CPU. |
| `cursor_blink_ms` | entier, de 50 à 5000 | `530` | Demi-période du clignotement, en millisecondes. |
| `highlight_current_line` | `true`, `false` | `true` | Surligne la ligne du curseur principal. |
| `line_numbers` | `"absolute"`, `"relative"`, `"off"` | `"absolute"` | Numéros de ligne dans la gouttière. `relative` : distance à la ligne du curseur, et numéro réel sur cette ligne. Avec `off`, la colonne des marqueurs (diagnostics, Git, points d'arrêt) reste réservée. |
| `theme` | identifiant de thème | `"padparadscha"` | Thème de couleurs : `padparadscha`, `clair`, `sombre`, ou un thème de l'utilisateur (voir [Thèmes](#thèmes)). |

### `[notifications]`

| Option | Valeurs | Défaut | Effet |
|---|---|---|---|
| `timeout_ms` | entier ≥ 0 | `4000` | Durée d'affichage des informations et avertissements, en millisecondes. `0` : jusqu'à un clic. |
| `error_timeout_ms` | entier ≥ 0 | `8000` | Durée d'affichage des erreurs. `0` : jusqu'à un clic. |

Les notifications s'affichent en bas à droite (quatre au plus) ; un clic en retire une.
L'historique se consulte avec `notifications list`.

### `[status_bar]`

| Option | Valeurs | Défaut | Effet |
|---|---|---|---|
| `left` | liste d'éléments | `["file", "modified", "keys"]` | Éléments à gauche de la barre d'état, dans l'ordre. |
| `right` | liste d'éléments | `["selections", "position", "line_ending", "encoding", "language"]` | Éléments à droite. |

Éléments : `file` (nom du fichier), `modified` (● si modifié), `keys` (séquence de touches en
cours, `ctrl+k …`), `selections` (nombre de curseurs, s'il y en a plusieurs), `position` (ligne et
colonne, en caractères affichés, et taille de la sélection), `line_ending` (`LF`, `CRLF`),
`encoding` (`UTF-8`, `UTF-8 BOM`), `language` (d'après l'extension). Un nom inconnu est signalé
et ignoré. Les plugins pourront ajouter leurs propres éléments.

### `[tabs]`

| Option | Valeurs | Défaut | Effet |
|---|---|---|---|
| `preview` | `true`, `false` | `false` | Onglet d'aperçu : un fichier ouvert depuis l'ouverture rapide ou l'explorateur remplace l'onglet d'aperçu de la zone au lieu d'en ajouter un. L'onglet d'aperçu (nom en gris) est gardé dès que le document est modifié, avec `tab keep` ou un double clic. |

### `[keys]`

Raccourcis : `"touches" = "ligne de commande"`. Ils s'ajoutent aux raccourcis par défaut et les
remplacent pour les mêmes touches ; une chaîne vide retire un raccourci par défaut.

- Combinaison : `ctrl`, `alt`, `shift`, `meta` (alias `cmd`, `super`, `win`) et une touche,
  séparés par `+` : `"ctrl+alt+t"`.
- `mod` : Ctrl sous Windows et Linux, Cmd sous macOS : `"mod+s"`.
- Séquence : combinaisons séparées par des espaces : `"ctrl+k ctrl+c"`.
- Touches spéciales : `enter`, `tab`, `backspace`, `delete`, `escape`, `space`, `left`, `right`,
  `up`, `down`, `home`, `end`, `pageup`, `pagedown`, `insert`, `f1` à `f24`, `plus`.
- Une séquence qui en masque une autre (`ctrl+k` et `ctrl+k ctrl+c`) est signalée au chargement.

Les commandes disponibles sont décrites dans [`commands.md`](commands.md).

#### Raccourcis par défaut

Les mêmes commandes sur les trois OS : Cmd sous macOS là où Windows et Linux utilisent Ctrl, et
Option pour les déplacements par mot. `keys list` affiche les raccourcis actifs (`*` : ceux de la
configuration), `keys describe <touches>` la commande d'une combinaison.

<!-- raccourcis par défaut : début -->
| Commande | Windows et Linux | macOS |
|---|---|---|
| `edit move left` | `←` | `←` |
| `edit move right` | `→` | `→` |
| `edit move up` | `↑` | `↑` |
| `edit move down` | `↓` | `↓` |
| `edit move left --extend` | `Maj+←` | `Maj+←` |
| `edit move right --extend` | `Maj+→` | `Maj+→` |
| `edit move up --extend` | `Maj+↑` | `Maj+↑` |
| `edit move down --extend` | `Maj+↓` | `Maj+↓` |
| `edit move line-start` | `Début` | `Début`, `Cmd+←` |
| `edit move line-end` | `Fin` | `Fin`, `Cmd+→` |
| `edit move line-start --extend` | `Maj+Début` | `Maj+Début` |
| `edit move line-end --extend` | `Maj+Fin` | `Maj+Fin` |
| `edit move doc-start` | `Ctrl+Début` | `Cmd+Début`, `Cmd+↑` |
| `edit move doc-end` | `Ctrl+Fin` | `Cmd+Fin`, `Cmd+↓` |
| `edit move page-up` | `Page préc.` | `Page préc.` |
| `edit move page-down` | `Page suiv.` | `Page suiv.` |
| `edit move page-up --extend` | `Maj+Page préc.` | `Maj+Page préc.` |
| `edit move page-down --extend` | `Maj+Page suiv.` | `Maj+Page suiv.` |
| `edit newline` | `Entrée` | `Entrée` |
| `edit insert '\t'` | `Tab` | `Tab` |
| `edit delete-backward` | `Retour arrière` | `Retour arrière` |
| `edit delete-forward` | `Suppr` | `Suppr` |
| `edit keep-primary` | `Échap` | `Échap` |
| `edit select-all` | `Ctrl+A` | `Cmd+A` |
| `edit copy` | `Ctrl+C`, `Ctrl+Inser` | `Cmd+C` |
| `edit cut` | `Ctrl+X`, `Maj+Suppr` | `Cmd+X` |
| `edit paste` | `Ctrl+V`, `Maj+Inser` | `Cmd+V` |
| `edit undo` | `Ctrl+Z` | `Cmd+Z` |
| `edit redo` | `Ctrl+Maj+Z`, `Ctrl+Y` | `Cmd+Maj+Z` |
| `ui command-line` | `Ctrl+;` | `Cmd+;` |
| `file save` | `Ctrl+S` | `Cmd+S` |
| `ui command-line 'file save-as '` | `Ctrl+Maj+S` | `Cmd+Maj+S` |
| `ui command-line 'file open '` | `Ctrl+O` | `Cmd+O` |
| `panel close` | `Ctrl+W` | `Cmd+W` |
| `tab next` | `Ctrl+Tab`, `Ctrl+Page suiv.` | `Ctrl+Tab`, `Ctrl+Page suiv.` |
| `tab previous` | `Ctrl+Maj+Tab`, `Ctrl+Page préc.` | `Ctrl+Maj+Tab`, `Ctrl+Page préc.` |
| `tab move-right` | `Ctrl+Maj+Page suiv.` | `Ctrl+Maj+Page suiv.` |
| `tab move-left` | `Ctrl+Maj+Page préc.` | `Ctrl+Maj+Page préc.` |
| `split right` | `Ctrl+\` | `Cmd+\` |
| `split down` | `Ctrl+K Ctrl+\` | `Cmd+K Cmd+\` |
| `panel focus left` | `Ctrl+K Ctrl+←` | `Cmd+K Cmd+←` |
| `panel focus right` | `Ctrl+K Ctrl+→` | `Cmd+K Cmd+→` |
| `panel focus up` | `Ctrl+K Ctrl+↑` | `Cmd+K Cmd+↑` |
| `panel focus down` | `Ctrl+K Ctrl+↓` | `Cmd+K Cmd+↓` |
| `file close` | `Ctrl+K Ctrl+W` | `Cmd+K Cmd+W` |
| `view zoom-in` | `Ctrl+=`, `Ctrl+Maj+=`, `Ctrl++` | `Cmd+=`, `Cmd+Maj+=`, `Cmd++` |
| `view zoom-out` | `Ctrl+-` | `Cmd+-` |
| `view zoom-reset` | `Ctrl+0` | `Cmd+0` |
| `edit move word-left` | `Ctrl+←` | `Option+←` |
| `edit move word-right` | `Ctrl+→` | `Option+→` |
| `edit move word-left --extend` | `Ctrl+Maj+←` | `Option+Maj+←` |
| `edit move word-right --extend` | `Ctrl+Maj+→` | `Option+Maj+→` |
| `edit add-cursor up` | `Ctrl+Alt+↑` | `Option+Cmd+↑` |
| `edit add-cursor down` | `Ctrl+Alt+↓` | `Option+Cmd+↓` |
<!-- raccourcis par défaut : fin -->

## Thèmes

Thèmes intégrés : **Padparadscha** (par défaut, sombre), **Clair** et **Sombre**. `theme list`
les liste, `theme set <nom>` change de thème pour la session ; `[view] theme` le garde.

Un thème de l'utilisateur est un fichier `.toml` du dossier `themes/`, à côté de `config.toml` ;
son identifiant est le nom du fichier sans l'extension (`themes/rubis.toml` → `rubis`). Chaque
couleur s'écrit `"#rrggbb"` ou `"#rrggbbaa"` ; une couleur absente est reprise du thème `base`
(Padparadscha par défaut). Une couleur invalide ou inconnue est signalée et ignorée.

```toml
name = "Rubis"
base = "sombre"          # facultatif

[ui]
accent = "#e0115f"
cursor = "#e0115f"
selection = "#e0115f44"

[syntax]
keyword = "#ff5c8a"
```

| Section | Couleurs |
|---|---|
| `[ui]` | `background`, `foreground`, `cursor`, `selection`, `current_line`, `line_number`, `line_number_active`, `panel`, `popup`, `border`, `accent`, `muted`, `list_selection`, `info`, `warning`, `error` |
| `[syntax]` | `comment`, `keyword`, `string`, `number`, `constant`, `function`, `type`, `variable`, `operator`, `punctuation` |

Les fichiers des thèmes intégrés servent d'exemples complets :
[`crates/ui/themes/`](../crates/ui/themes/).

## Fichier par défaut

```toml
# Configuration de CuprIDE.
# Les lignes commentées donnent les valeurs par défaut.

[editor]
# Fin de ligne des nouveaux documents : "auto" (LF), "lf" ou "crlf".
# Un fichier existant garde toujours ses fins de ligne.
# line_ending = "auto"

# Pause, en millisecondes, au-delà de laquelle une nouvelle frappe commence un nouveau bloc
# d'annulation. 0 : chaque frappe est annulée séparément.
# undo_group_delay_ms = 1000

# Sélection primaire (Linux, X11 et Wayland) : le texte sélectionné est copié dans la sélection
# primaire, et le clic du milieu le colle. Sans effet sous Windows et macOS.
# primary_selection = false

[font]
# Famille de police : "monospace" (police à chasse fixe du système) ou un nom de famille
# installée, par exemple "JetBrains Mono" ou "Fira Code" (ligatures).
# family = "monospace"

# Taille en points (pixels logiques, multipliés par l'échelle de l'écran).
# size = 14.0

# Hauteur de ligne, en multiple de la taille.
# line_height = 1.4

[view]
# Clignotement du curseur. Il s'arrête de lui-même après quelques secondes sans saisie, pour que
# l'IDE ne consomme aucun CPU au repos.
# cursor_blink = true

# Demi-période du clignotement, en millisecondes.
# cursor_blink_ms = 530

# Surligne la ligne du curseur principal.
# highlight_current_line = true

# Numéros de ligne : "absolute", "relative" (distance au curseur, numéro réel sur sa ligne)
# ou "off".
# line_numbers = "absolute"

# Thème : "padparadscha", "clair", "sombre", ou un thème du dossier themes/ de la configuration
# (son nom de fichier, sans .toml).
# theme = "padparadscha"

[notifications]
# Durée d'affichage des informations et avertissements, en millisecondes. 0 : jusqu'à un clic.
# timeout_ms = 4000

# Durée d'affichage des erreurs, en millisecondes. 0 : jusqu'à un clic.
# error_timeout_ms = 8000

[status_bar]
# Éléments de la barre d'état, à gauche et à droite : "file" (nom du fichier), "modified"
# (modifications non enregistrées), "keys" (séquence de touches en cours), "selections" (nombre
# de curseurs), "position" (ligne et colonne), "line_ending", "encoding", "language".
# left = ["file", "modified", "keys"]
# right = ["selections", "position", "line_ending", "encoding", "language"]

[tabs]
# Onglet d'aperçu : un fichier ouvert depuis l'ouverture rapide ou l'explorateur remplace l'onglet
# d'aperçu de la zone au lieu d'en ajouter un ; l'onglet est gardé dès qu'on modifie le document
# (ou avec « tab keep »).
# preview = false

[keys]
# Raccourcis : touches = "ligne de commande". Ils s'ajoutent aux raccourcis par défaut et les
# remplacent pour les mêmes touches. Une chaîne vide retire un raccourci par défaut.
# "mod" vaut Ctrl sous Windows et Linux, Cmd sous macOS.
# "mod+shift+s" = "file save-as"
# "ctrl+k ctrl+c" = "edit insert '// '"
# "mod+k mod+w" = ""
```
