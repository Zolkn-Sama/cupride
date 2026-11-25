# Référence des commandes

Toute action de CuprIDE est une commande. Une même commande est accessible depuis :

- la **ligne de commande interne** (`:`) ;
- la **palette** de commandes, avec recherche floue ;
- un **raccourci**, qui n'est qu'une ligne de commande pré-remplie ;
- la **CLI externe**, depuis un terminal système : `cupride <groupe> <commande> [arguments]` ;
- les **plugins**.

## Lancement

```
cupride [chemin…]
```

| Appel | Effet |
|---|---|
| `cupride` | Document vide. |
| `cupride src/main.rs` | Ouvre le fichier. Un fichier inexistant ouvre un nouveau document avec ce chemin, créé au premier enregistrement. |
| `cupride .` | Ouvre le dossier comme projet (son nom s'affiche dans le titre). |
| `cupride . a.rs b.rs` | Dossier du projet et fichiers ; le premier fichier est actif. |
| `cupride --help`, `cupride --version` | Aide, version. |

Un fichier illisible (binaire, non UTF-8) n'empêche pas de démarrer : il est signalé dans le
titre de la fenêtre et dans le journal. Une option inconnue arrête le lancement avec le code 2.

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
| `file open <chemin> [--preview]` | Ouvre un fichier et en fait le document actif, dans un onglet de la zone active (`--preview` : dans l'onglet d'aperçu, voir [`tab`](#tab--onglets)). Un fichier déjà ouvert n'est pas dupliqué : son document redevient actif. Un fichier inexistant est créé au premier enregistrement. Fichier binaire ou non UTF-8 : refusé, jamais modifié. |
| `file save` | Enregistre le document actif, en conservant ses fins de ligne. Un document sans fichier demande `file save-as`. |
| `file save-as <chemin>` | Enregistre le document actif sous `<chemin>`, qui devient son fichier. |
| `file close [--force]` | Ferme le document actif, et tous les panneaux qui l'affichent. S'il a des modifications non enregistrées, la commande échoue, sauf avec `--force`. |
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

## `ui` — interface

| Commande | Description |
|---|---|
| `ui command-line [texte]` | Ouvre la ligne de commande (raccourci `mod+;`), éventuellement préremplie. |
| `ui palette` | Ouvre la palette de commandes (`mod+shift+p`, `F1`). |
| `ui quick-open` | Ouvre un fichier du projet en tapant une partie de son chemin (`mod+p`). |

Ligne de commande : Entrée exécute la commande. Sans résultat, la ligne se ferme ; en cas
d'erreur, le message s'affiche au-dessus et le texte reste, pour être corrigé ; une sortie
(`file list`) s'affiche au-dessus d'une ligne vidée. Échap ferme la ligne. Édition : flèches,
mot par mot (Alt sous macOS, Ctrl ailleurs), Début et Fin, retour arrière et suppression (par
mot avec le même modificateur), sélection avec Maj, `mod+a`, `mod+c`, `mod+x`, `mod+v`. Les
autres raccourcis restent actifs pendant la saisie.

- **Historique** : flèches haut et bas ; la saisie en cours est retrouvée en redescendant. Les 500
  dernières commandes sont gardées entre les sessions, dans le dossier de données de l'OS
  (`command-history`).
- **Complétion** : Tab complète le mot sous le curseur : groupes, commandes, options, valeurs
  possibles d'un argument, chemins (relatifs au dossier courant). Plusieurs propositions : leur
  début commun est complété et elles sont listées sous la ligne ; Tab de nouveau les parcourt.
- **Aide** : dès que la commande est reconnue, son utilisation et sa description s'affichent sous
  la ligne.

Palette : recherche floue (`nucleo`) parmi toutes les commandes, avec leur description et leur
raccourci ; les commandes récemment utilisées (ligne de commande ou palette) passent en tête.
Flèches haut et bas pour choisir, Entrée ou clic pour exécuter, Échap pour fermer. Une commande
qui attend des arguments (`file open <chemin>`) ouvre la ligne de commande, préremplie.

Ouverture rapide : recherche floue sur les chemins des fichiers du projet (le dossier ouvert avec
`cupride <dossier>`, sinon le dossier courant), les documents déjà ouverts en tête. Les fichiers
ignorés par `.gitignore` et `.ignore`, et les fichiers cachés, sont exclus. L'indexation se fait
en arrière-plan dès l'ouverture de la fenêtre, puis à chaque ouverture rapide (au plus toutes les
10 secondes) : la liste s'allonge au fil de l'indexation, l'interface n'attend jamais.

## `notifications` — notifications

| Commande | Description |
|---|---|
| `notifications list` | Affiche l'historique (les 200 dernières), de la plus récente à la plus ancienne, avec leur gravité et leur âge. |
| `notifications clear` | Vide l'historique. |

Le résultat d'une commande lancée par un raccourci s'affiche en notification (information), une
erreur aussi (erreur), comme les avertissements de configuration et de fermeture.

## `keys` — raccourcis

| Commande | Description |
|---|---|
| `keys list [filtre]` | Affiche les raccourcis actifs, triés par commande ; `*` marque ceux de la configuration. Avec un filtre, seulement ceux dont la commande ou les touches le contiennent. |
| `keys describe <touches>` | Indique la commande exécutée par une combinaison (`mod+s`) ou une séquence (`"ctrl+k ctrl+w"`). |

## `theme` — thèmes

| Commande | Description |
|---|---|
| `theme set <nom>` | Change de thème pour la session (`[view] theme` dans la configuration pour le garder). Un nom inconnu est refusé, avec le nom le plus proche. |
| `theme list` | Liste les thèmes disponibles ; `>` marque le thème en cours. |

## `window` — fenêtres

| Commande | Description | Issue |
|---|---|---|
| `window new [--monitor N]` | Ouvre une nouvelle fenêtre, éventuellement sur l'écran `N` | WIN |
| `window close` | Ferme la fenêtre active | WIN |
| `window move --monitor N` | Déplace la fenêtre active sur l'écran `N` | WIN |
| `window list-monitors` | Liste les écrans : nom, résolution, échelle | WIN |

## `split` — diviser l'espace

Chaque commande divise la zone active en deux : le document actif s'ouvre aussi dans la nouvelle
zone, qui prend le focus. C'est le même document en mémoire, avec ses propres curseurs et son
propre défilement dans chaque zone. Sans document, la nouvelle zone reçoit un document vide.

| Commande | Description |
|---|---|
| `split right` | Nouvelle zone à droite de la zone active (`mod+\`). |
| `split down` | Nouvelle zone sous la zone active (`mod+k mod+\`). |
| `split left` | Nouvelle zone à gauche de la zone active. |
| `split up` | Nouvelle zone au-dessus de la zone active. |

Une zone est divisée dans le sens de son split quand elle en a déjà un : trois `split right` de
suite donnent quatre colonnes, pas des splits imbriqués.

## `tab` — onglets

Chaque zone est un groupe d'onglets ; ces commandes agissent sur la zone active. Le titre d'un
onglet est le nom de son document, suivi de ● s'il a des modifications non enregistrées. Un clic
affiche l'onglet, un clic du milieu le ferme (comme `panel close`).

| Commande | Description |
|---|---|
| `tab next` | Affiche l'onglet suivant ; après le dernier, le premier (`ctrl+tab`, `ctrl+pagedown`). |
| `tab previous` | Affiche l'onglet précédent (`ctrl+shift+tab`, `ctrl+pageup`). |
| `tab move-left` | Déplace l'onglet actif d'un rang vers la gauche (`ctrl+shift+pageup`). |
| `tab move-right` | Déplace l'onglet actif d'un rang vers la droite (`ctrl+shift+pagedown`). |
| `tab keep` | Garde l'onglet d'aperçu actif (aussi : double clic sur l'onglet, ou modifier le document). |

**Onglet d'aperçu** (option `[tabs] preview`, désactivée par défaut) : un fichier ouvert depuis
l'ouverture rapide (ou `file open --preview`) remplace l'onglet d'aperçu de la zone au lieu d'en
ajouter un. Son titre est en italique et en gris. Le document qu'il affichait est fermé s'il
n'est affiché nulle part ailleurs.

## `panel` — panneaux

Un panneau a un type : `editor` (un document), `explorer` (fichiers du projet), ou un type fourni
par un plugin.

| Commande | Description |
|---|---|
| `panel open <type> [--split <direction>]` | Ouvre un panneau du type indiqué : en onglet dans la zone active, ou dans une nouvelle zone du côté `left`, `right`, `up` ou `down`. `editor` ouvre le document actif. Un type inconnu est refusé, avec le nom le plus proche. |
| `panel close [--force]` | Ferme le panneau actif (`mod+w`) ; sa zone disparaît si elle est vide, et sa place revient aux zones voisines. Son document est fermé s'il n'est plus affiché ailleurs : s'il a des modifications non enregistrées, la commande échoue, sauf avec `--force`. |
| `panel focus <direction>` | Donne le focus à la zone voisine : `left`, `right`, `up` ou `down` (`mod+k` puis `mod+←`, `mod+→`, `mod+↑`, `mod+↓`). Parmi les zones de ce côté, celle qui touche et recouvre le plus la zone active. |
| `panel resize <sens>` | Agrandit ou réduit la zone active de 5 % de la fenêtre : `wider`, `narrower` (largeur), `taller`, `shorter` (hauteur). La place est prise à la zone voisine (la suivante, ou la précédente pour la dernière) ; chaque zone garde une taille minimale (sa barre d'onglets et trois lignes). Raccourcis : `mod+k` puis `mod+shift+→`, `←`, `↓`, `↑`. |
| `panel list` | Liste les panneaux, zone par zone : `>` marque le panneau actif. |

Le panneau actif se reconnaît à son onglet, souligné en haut de la couleur d'accent ; les titres
des autres zones sont en gris. Un clic dans une zone lui donne le focus. Les séparations entre
les zones se glissent à la souris (le pointeur change au-dessus), dans la limite de la taille
minimale des zones.

Exemples :

```
panel open explorer --split left
panel open editor --split down
```

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
