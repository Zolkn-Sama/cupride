# Référence des commandes

Toute action de CuprIDE est une commande. Une même commande est accessible depuis :

- la **ligne de commande interne** (`:`) ;
- la **palette** de commandes, avec recherche floue ;
- un **raccourci**, qui n'est qu'une ligne de commande pré-remplie ;
- la **CLI externe**, depuis un terminal système : `cupride <groupe> <commande> [arguments]` ;
- les **plugins**.

## Syntaxe

```
<groupe> <commande> [arguments] [--options]
```

- Les arguments sont séparés par des espaces ; les guillemets regroupent un argument qui en contient
  (`file open "mon fichier.rs"`).
- Chaque commande dispose d'une aide intégrée : `<groupe> <commande> --help`.
- Une commande renvoie un succès, ou une erreur affichée dans la barre d'état.

> Les commandes ci-dessous sont **prévues** : chacune est documentée ici au moment de son
> implémentation, avec ses arguments et ses options. La colonne « Issue » renvoie à la story
> correspondante.

## `file` — fichiers

Les commandes agissent sur le **document actif**. L'enregistrement est atomique : un plantage
pendant l'écriture laisse toujours l'ancien fichier intact.

| Commande | Description |
|---|---|
| `file open <chemin>` | Ouvre un fichier et en fait le document actif. Un fichier déjà ouvert n'est pas dupliqué : son document redevient actif. Un fichier inexistant est créé au premier enregistrement. Fichier binaire ou non UTF-8 : refusé, jamais modifié. |
| `file save` | Enregistre le document actif, en conservant ses fins de ligne. Un document sans fichier demande `file save-as`. |
| `file save-as <chemin>` | Enregistre le document actif sous `<chemin>`, qui devient son fichier. |
| `file close [--force]` | Ferme le document actif. S'il a des modifications non enregistrées, la commande échoue, sauf avec `--force`. |
| `file reload [--force]` | Relit le document actif depuis le disque ; l'historique est vidé. Mêmes règles que `file close` pour les modifications non enregistrées. |
| `file list` | Liste les documents ouverts : `>` marque le document actif, `*` un document modifié. |

Exemples :

```
file open src/main.rs
file open "notes de réunion.md"
file save-as '/tmp/copie.rs'
file close --force
```

## `edit` — édition

Toute action clavier de l'éditeur correspond à l'une de ces commandes. Elles agissent sur
**toutes les sélections** du document actif (multi-curseur) ; une sélection vide est un curseur.
Les frappes et effacements consécutifs sont annulés par blocs (voir `edit undo`).

| Commande | Description |
|---|---|
| `edit insert <texte>` | Insère `<texte>` à chaque curseur ; une sélection non vide est remplacée. |
| `edit newline` | Insère un saut de ligne, avec la fin de ligne du document (`\n` ou `\r\n`). |
| `edit delete-backward` | Supprime chaque sélection, ou le caractère (graphème) avant chaque curseur. |
| `edit delete-forward` | Supprime chaque sélection, ou le caractère (graphème) après chaque curseur. |
| `edit undo` | Annule la dernière modification et restaure les sélections. |
| `edit redo` | Rétablit la dernière modification annulée. |
| `edit move <direction> [--extend]` | Déplace les curseurs ; avec `--extend`, étend les sélections. |
| `edit select-all` | Sélectionne tout le texte. |
| `edit collapse` | Réduit chaque sélection à un curseur sur sa tête. |
| `edit keep-primary` | Ne garde que la sélection principale. |
| `edit add-cursor <up\|down>` | Ajoute un curseur sur la ligne au-dessus ou en dessous du curseur principal. |
| `edit copy` | Copie les sélections dans le presse-papiers : un fragment par sélection, séparés par des fins de ligne. |
| `edit cut` | Copie les sélections, puis les supprime. |
| `edit paste [--primary]` | Colle le presse-papiers à chaque curseur, avec les fins de ligne du document. Avec plusieurs curseurs, chacun reçoit son fragment si les nombres correspondent (fragments de la dernière copie faite dans l'IDE, ou sinon lignes du texte collé). `--primary` : sélection primaire de X11 et Wayland. |

Directions de `edit move` :

| Direction | Déplacement |
|---|---|
| `left`, `right` | Caractère (graphème) précédent ou suivant ; sans `--extend`, une sélection est d'abord réduite à son bord. |
| `up`, `down` | Ligne précédente ou suivante, à la même colonne si possible, sinon en fin de ligne. |
| `word-left`, `word-right` | Début du mot précédent, fin du mot suivant. |
| `line-start`, `line-end` | Début ou fin de la ligne (avant la fin de ligne). |
| `doc-start`, `doc-end` | Début ou fin du texte. |
| `page-up`, `page-down` | Une page plus haut ou plus bas (le nombre de lignes visibles), à la même colonne si possible. |

Saisie au clavier : le texte imprimable tapé sans Ctrl ni Cmd (y compris les accents composés
par une touche morte, AltGr sous Windows et le texte validé par une méthode de saisie chinoise,
japonaise ou coréenne) est inséré avec `edit insert`. Toutes les autres touches passent par le
keymap ([`configuration.md`](configuration.md), section `[keys]`).

Souris : clic pour placer le curseur, glisser pour sélectionner, double clic pour un mot (glisser
ensuite étend mot par mot), triple clic pour une ligne (puis ligne par ligne), Maj+clic pour
étendre la sélection, Alt+clic pour ajouter un curseur.

Exemples :

```
edit insert "fn main() {}"
edit move word-right --extend
edit add-cursor down
```

## `config` — configuration

Voir [`configuration.md`](configuration.md) pour les options.

| Commande | Description |
|---|---|
| `config open` | Ouvre le fichier de configuration dans l'IDE ; s'il n'existe pas, il est prérempli avec les options par défaut. |
| `config reload` | Relit le fichier de configuration et l'applique ; affiche les erreurs (avec ligne et colonne) et les avertissements. |

## `view` — affichage

| Commande | Description |
|---|---|
| `view zoom-in` | Agrandit l'interface d'un cran (raccourcis `mod+=`, `mod+plus`). |
| `view zoom-out` | Réduit l'interface d'un cran (`mod+-`). |
| `view zoom-reset` | Revient à 100 % (`mod+0`). |

Crans : 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250 et 300 %. Le zoom multiplie
l'échelle de l'écran : le texte est remis en forme à la nouvelle taille, net, sans redémarrage ;
le haut de la vue reste sur le même texte. Un changement d'écran (densité différente) est appliqué
de la même façon, à chaud.

## `window` — fenêtres

| Commande | Description | Issue |
|---|---|---|
| `window new [--monitor N]` | Ouvre une nouvelle fenêtre, éventuellement sur l'écran `N` | WIN |
| `window close` | Ferme la fenêtre active | WIN |
| `window move --monitor N` | Déplace la fenêtre active sur l'écran `N` | WIN |
| `window list-monitors` | Liste les écrans : nom, résolution, échelle | WIN |

## `panel` — panneaux

Ouverture, fermeture, découpage et déplacement des panneaux (épic Layout).

## `layout` — layouts nommés

| Commande | Description | Issue |
|---|---|---|
| `layout save <nom>` | Enregistre le layout courant | LAYOUT |
| `layout load <nom>` | Restaure un layout enregistré | LAYOUT |
| `layout list` | Liste les layouts enregistrés | LAYOUT |
| `layout delete <nom>` | Supprime un layout enregistré | LAYOUT |

## `pkg` — paquets

| Commande | Description | Issue |
|---|---|---|
| `pkg install <paquet>` | Installe un paquet et ses dépendances | PKG |
| `pkg remove <paquet>` | Désinstalle un paquet | PKG |
| `pkg update [paquet]` | Met à jour un paquet, ou tous | PKG |
| `pkg search <texte>` | Cherche dans le registre | PKG |
| `pkg list` | Liste les paquets installés | PKG |
| `pkg info <paquet>` | Affiche le détail d'un paquet | PKG |
| `pkg rollback <paquet>` | Revient à la version précédente | PKG |
| `pkg publish` | Vérifie, empaquette et signe un paquet pour le registre | PKG |
