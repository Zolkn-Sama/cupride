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

[keys]
# Raccourcis : touches = "ligne de commande". Ils s'ajoutent aux raccourcis par défaut et les
# remplacent pour les mêmes touches. Une chaîne vide retire un raccourci par défaut.
# "mod" vaut Ctrl sous Windows et Linux, Cmd sous macOS.
# "mod+shift+s" = "file save-as"
# "ctrl+k ctrl+c" = "edit insert '// '"
# "mod+k mod+w" = ""
```
