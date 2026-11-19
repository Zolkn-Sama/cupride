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

[keys]
# Raccourcis : touches = "ligne de commande". Ils s'ajoutent aux raccourcis par défaut et les
# remplacent pour les mêmes touches. Une chaîne vide retire un raccourci par défaut.
# "mod" vaut Ctrl sous Windows et Linux, Cmd sous macOS.
# "mod+shift+s" = "file save-as"
# "ctrl+k ctrl+c" = "edit insert '// '"
# "mod+k mod+w" = ""
```
