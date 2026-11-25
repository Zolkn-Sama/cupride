# Référence des commandes

Toute action de CuprIDE est une commande. Une même commande est accessible depuis :

- la **ligne de commande interne** (`:`) ;
- la **palette** de commandes, avec recherche floue ;
- un **raccourci**, qui n'est qu'une ligne de commande pré-remplie ;
- la **CLI externe**, depuis un terminal système : `cupride <groupe> <commande> [arguments]` ;
- les **plugins**.

## Lancement

```
cupride [options] [chemin…]
cupride [--window N] <groupe> <commande> [arguments]
```

| Appel | Effet |
|---|---|
| `cupride` | Session restaurée, sinon document vide. |
| `cupride src/main.rs` | Ouvre le fichier. Un fichier inexistant ouvre un nouveau document avec ce chemin, créé au premier enregistrement. |
| `cupride .` | Ouvre le dossier comme projet (son nom s'affiche dans le titre). |
| `cupride . a.rs b.rs` | Dossier du projet et fichiers ; le premier fichier est actif. |
| `cupride --no-restore` | Démarre sans restaurer l'espace de travail du projet. |
| `cupride --window 2 a.rs` | Ouvre dans la fenêtre 2 de l'instance en marche. |
| `cupride --wait COMMIT_EDITMSG` | Ouvre le fichier et attend qu'il soit fermé avant de rendre la main : CuprIDE peut servir d'éditeur à Git (`git config --global core.editor "cupride --wait"`). Sans instance en marche, CuprIDE s'arrête à la fermeture du fichier, comme un éditeur de terminal. |
| `cupride panel focus left` | Exécute une commande dans l'instance en marche, avec la même syntaxe que la ligne de commande interne. |
| `cupride --new-instance [--config-dir dossier]` | Lance une instance indépendante, pour essayer une configuration sans toucher à sa session : elle ne passe pas par l'instance en marche et ne lui répond pas. Son dossier : `config.toml`, `themes/`, et ses données (session, layouts, historique) dans `data/`. Sans `--config-dir`, un dossier temporaire vierge, supprimé à la sortie. |
| `cupride --help`, `cupride --version` | Aide, version. |

**Instance unique** : un seul CuprIDE tourne par utilisateur ; toutes ses fenêtres partagent les
documents. Lancer `cupride` quand il tourne déjà lui transmet la ligne de commande (les chemins
relatifs partent du dossier du terminal) : les fichiers s'ouvrent dans sa fenêtre active, qui
passe au premier plan, et le nouveau processus se termine aussitôt, avec le code de sortie que
l'instance renvoie. La communication passe par un socket local privé (Unix : dans un dossier
`0700` de l'utilisateur, sous `$XDG_RUNTIME_DIR` ou le dossier temporaire ; Windows : un tube
nommé propre à l'utilisateur, fermé aux autres utilisateurs en écriture et aux clients distants).
Après un plantage, le socket orphelin est détecté et remplacé au lancement suivant.

**Commandes depuis le terminal** : un premier argument qui est un groupe de commandes (`file`,
`panel`, `window`…) fait de la suite une commande interne, exécutée dans l'instance (dans la
fenêtre `--window N` si elle est donnée). Son message s'affiche dans le terminal ; une erreur
s'affiche sur la sortie d'erreur, et `cupride` sort avec le code 1 (2 pour une ligne de commande
mal formée). Les chemins relatifs partent du dossier du terminal. Un fichier qui porterait le nom
d'un groupe s'ouvre avec `./nom`.

```
cupride window list
cupride file open src/main.rs
cupride --window 2 panel move --window 1
```

L'espace de travail du projet (zones, onglets, fichiers, curseurs, défilement) est restauré tel
qu'il était à la fermeture, sauf avec `--no-restore` ou l'option `[session] restore = false` (voir
la [configuration](configuration.md#session)) ; les fichiers demandés s'y ajoutent, dans la zone
active. Un fichier supprimé entre-temps : son panneau est fermé, et une notification le signale.

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
| `file open <chemin> [--preview] [--line N [--column N]]` | Ouvre un fichier et en fait le document actif, dans un onglet de la zone active (`--preview` : dans l'onglet d'aperçu, voir [`tab`](#tab--onglets)) ; `--line` et `--column` (à partir de 1, colonne en caractères) y placent le curseur. Un fichier déjà ouvert n'est pas dupliqué : son document redevient actif. Un fichier inexistant est créé au premier enregistrement. Fichier binaire ou non UTF-8 : refusé, jamais modifié. |
| `file save [--force]` | Enregistre le document actif, en conservant ses fins de ligne. Un document sans fichier demande `file save-as`. Si le fichier a été modifié par un autre programme depuis l'ouverture, la commande refuse de l'écraser, sauf avec `--force`. |
| `file keep` | Garde la version de l'éditeur d'un fichier modifié ailleurs : le prochain enregistrement l'écrasera. |
| `file compare` | Ouvre, dans un nouveau document, les différences entre le fichier sur le disque (lignes `-`) et le document actif (lignes `+`). |
| `file save-as <chemin>` | Enregistre le document actif sous `<chemin>`, qui devient son fichier. |
| `file close [--force]` | Ferme le document actif, et tous les panneaux qui l'affichent. S'il a des modifications non enregistrées, la commande échoue, sauf avec `--force`. |
| `file reload [--force]` | Relit le document actif depuis le disque ; l'historique est vidé. Mêmes règles que `file close` pour les modifications non enregistrées. |
| `file list` | Liste les documents ouverts : `>` marque le document actif, `*` un document modifié. |

**Fichier modifié hors de l'IDE** : les dossiers des documents ouverts sont surveillés (`notify`),
et tout est revérifié quand l'IDE reprend le focus. Un document sans modification est rechargé.
Un document modifié n'est jamais écrasé : une notification propose `file compare`, `file keep`
(garder la sienne) ou `file reload --force` (prendre celle du disque). Un fichier supprimé ou
déplacé est signalé ; le document reste ouvert, et `file save` le recrée. Réécrire le même contenu
(ou seulement changer la date du fichier) ne déclenche rien.

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

## `search` — rechercher

| Commande | Description |
|---|---|
| `search find <motif> [--case] [--word] [--regex]` | Cherche le motif dans le document actif et sélectionne l'occurrence suivante, à partir du début de la sélection ; affiche sa position (`3/12`). Par défaut, la casse est ignorée ; `--case` la respecte, `--word` ne garde que les mots entiers, `--regex` lit le motif comme une expression régulière (syntaxe de la crate `regex`). |
| `search next` | Occurrence suivante ; après la dernière, la première (`F3` ; `Cmd+G` sous macOS). |
| `search previous` | Occurrence précédente (`Maj+F3` ; `Cmd+Maj+G` sous macOS). |
| `search replace <texte>` | Remplace l'occurrence sélectionnée par le texte, puis sélectionne la suivante. Avec `--regex` dans la recherche, `$1`, `${nom}` et `$0` reprennent les groupes de capture (`$$` pour un `$`) ; sinon le texte est pris tel quel. |
| `search replace-all <texte>` | Remplace toutes les occurrences, en une seule modification : un seul `edit undo` les rétablit toutes. |
| `search clear` | Arrête la recherche en cours. |
| `search project <motif> [--case] [--word] [--regex]` | Cherche dans tous les fichiers du projet (le dossier ouvert, sinon le dossier courant), en arrière-plan. Les résultats s'affichent au fil de l'eau dans le panneau de recherche, qui s'ouvre à gauche (en onglet à côté de l'explorateur s'il est ouvert). Les fichiers ignorés par `.gitignore` et `.ignore`, les fichiers cachés et les fichiers binaires sont sautés ; au-delà de 10 000 occurrences, la recherche s'arrête. |
| `search project-replace <texte>` | Aperçu : chaque ligne des résultats s'affiche dans le panneau telle qu'elle deviendrait. Rien n'est modifié. |
| `search project-apply` | Confirme le remplacement en aperçu. Un fichier ouvert est modifié dans son document (une seule modification, annulable, à enregistrer) ; les autres sont réécrits sur le disque. |
| `ui search-project` | Prépare `search project` dans la ligne de commande, avec la sélection (`mod+shift+f`). |
| `ui find` | Ouvre la barre de recherche (`mod+f`), préremplie avec la sélection (sur une ligne) ou le motif en cours. |
| `ui replace` | Ouvre la barre avec la ligne de remplacement (`Ctrl+H`, `Option+Cmd+F` sous macOS). |

**Panneau de recherche** : en tête, le bilan (« 12 résultats dans 3 fichiers ») ; puis chaque
fichier et ses lignes, les occurrences en couleur. Flèches, pages, Début et Fin pour se déplacer ;
Entrée ou double clic ouvre le fichier à la ligne et à la colonne de l'occurrence ; Espace ou clic
l'ouvre dans l'onglet d'aperçu.

**Barre de recherche** : en haut à droite de la zone active. La recherche est incrémentale :
chaque frappe sélectionne la première occurrence à partir du point de départ, et toutes les
occurrences sont surlignées. Entrée (ou ↓) : suivante ; Maj+Entrée (ou ↑) : précédente ; Échap :
fermer (le motif reste, `F3` continue). Options : `Alt+C` casse, `Alt+W` mot entier, `Alt+R`
expression régulière, ou un clic sur `Aa`, `mot`, `.*`. Une expression invalide est signalée dans
la barre. La recherche se fait dans un thread à part, sur une copie du document : l'interface ne
bloque jamais, même sur un fichier de 100 Mo. Ligne de remplacement : Tab passe d'un champ à
l'autre ; dans le champ de remplacement, Entrée remplace l'occurrence sélectionnée et passe à la
suivante, `mod+Entrée` remplace tout. Au-delà de 100 000 occurrences, seules les
100 000 premières sont gardées (« limite »).

## Lignes

Ces commandes agissent sur toutes les lignes touchées par les sélections (une sélection qui
finit au tout début d'une ligne ne la compte pas), en une seule modification annulable.

| Commande | Description |
|---|---|
| `edit goto <ligne[:colonne]>` | Place le curseur à la ligne et à la colonne (à partir de 1, colonne en caractères), bornées au texte. |
| `ui goto` | Prépare `edit goto` dans la ligne de commande (`Ctrl+G`). |
| `edit duplicate-lines` | Duplique les lignes ; les curseurs passent sur la copie du dessous (`Alt+Maj+↓`). |
| `edit move-lines <up\|down>` | Déplace les lignes d'une ligne vers le haut ou le bas ; les sélections les suivent (`Alt+↑`, `Alt+↓`). |
| `edit delete-lines` | Supprime les lignes (`mod+shift+k`). |
| `edit join-lines` | Joint chaque ligne à la suivante, avec une espace, sans l'indentation de la suivante ; sur une seule ligne, avec la suivante (`mod+j`). |
| `edit toggle-comment` | Commente les lignes non vides (au niveau d'indentation le plus faible du bloc), ou les décommente si elles le sont toutes (`mod+/`). La syntaxe vient du langage du fichier (`//`, `#`, `--`…), ou de l'option `[languages.<Nom>] comment` ; les paquets de langage la fourniront. |

## Indentation

| Commande | Description |
|---|---|
| `edit indent` | Sans sélection sur plusieurs lignes : insère jusqu'au prochain niveau, à chaque curseur (une tabulation, ou les espaces jusqu'à la prochaine colonne multiple de la largeur). Avec une sélection sur plusieurs lignes : un niveau au début de chaque ligne (`Tab`). |
| `edit outdent` | Retire un niveau (une tabulation, ou jusqu'à la largeur en espaces) au début de chaque ligne des sélections (`Maj+Tab`). |
| `edit indentation <tabs\|spaces> [--width N]` | Change l'indentation du document actif, pour la session : la configuration ne la change plus. |

L'indentation de chaque document vient de la configuration (`[editor] indent_style`,
`indent_width`, et `[languages.<Nom>]`) ; avec `auto`, elle est détectée à l'ouverture. La barre
d'état l'affiche (`Espaces : 4`). Entrée (`edit newline`) garde l'indentation de la ligne : la
nouvelle ligne commence par les mêmes espaces et tabulations, jusqu'au curseur.

## Multi-curseurs

| Commande | Description |
|---|---|
| `edit add-cursor up`, `down` | Ajoute un curseur sur la ligne au-dessus ou en dessous (aussi : Alt+clic). |
| `edit add-next-occurrence` | Sur un curseur, sélectionne le mot ; puis ajoute une sélection à l'occurrence suivante de son texte, en revenant au début (`mod+d`). |
| `edit select-all-occurrences` | Une sélection sur chaque occurrence du texte sélectionné ou du mot sous le curseur (`mod+shift+l`). |
| `edit keep-primary` | Revient à un seul curseur (`Échap`). |

Toutes les commandes d'édition (frappe, effacement, déplacements, couper, copier, coller)
agissent à chaque curseur.

## Sélection structurelle

D'après l'arbre syntaxique du document (langages qui ont une grammaire : Rust, C, Markdown).

| Commande | Description |
|---|---|
| `edit expand-selection` | Étend chaque sélection au nœud qui la contient : identifiant, expression, instruction, bloc, fonction… (`Alt+Maj+→` ; `Ctrl+Maj+Cmd+→` sous macOS). |
| `edit shrink-selection` | Réduit chaque sélection : défait les extensions précédentes dans l'ordre, sinon sélectionne le nœud enfant qui contient le curseur (`Alt+Maj+←` ; `Ctrl+Maj+Cmd+←`). |
| `edit next-function` | Chaque curseur va au début de la fonction suivante ; en Markdown, au titre suivant (`Ctrl+Maj+↓`). |
| `edit previous-function` | Au début de la fonction (ou du titre) précédente (`Ctrl+Maj+↑`). |

## Parenthèses et paires

La parenthèse (crochet, accolade) à côté du curseur principal et sa correspondante sont
surlignées. La correspondance vient de l'arbre syntaxique quand il y en a un (les parenthèses des
chaînes et des commentaires ne comptent pas), sinon du texte (64 Ko au plus de part et d'autre).

| Commande | Description |
|---|---|
| `edit goto-bracket` | Chaque curseur va à la parenthèse qui correspond à celle à côté de lui (`Ctrl+Maj+\` ; `Cmd+Maj+\`). |
| `edit type <texte>` | Tape du texte comme au clavier (c'est ce que fait la frappe) : avec `[editor] auto_close`, `(` insère `()`, une fermante devant la même fermante passe par-dessus, une sélection est entourée. `edit insert` insère le texte tel quel. |

Retour arrière entre deux caractères d'une paire vide (`(|)`) efface les deux.

## `recovery` — récupération après plantage

Les documents qui ont des modifications non enregistrées sont copiés toutes les 15 secondes (s'ils
ont changé) dans le dossier `recovery/` des données de l'OS — jamais dans les fichiers du projet.
À la fermeture normale, ces copies sont effacées. Si CuprIDE s'est arrêté brutalement, une
notification le signale au lancement suivant.

| Commande | Description |
|---|---|
| `recovery list` | Liste les documents retrouvés. |
| `recovery restore` | Restaure chaque document : le texte retrouvé remplace celui du fichier, en une modification annulable, non enregistrée (à vérifier, puis à enregistrer). Un document sans fichier devient un nouveau document. |
| `recovery discard` | Oublie les modifications retrouvées. |

## `fold` — repliage

Les blocs repliables viennent de l'arbre syntaxique : blocs, listes d'arguments ou de champs,
commentaires sur plusieurs lignes, directives `#if` en C, sections, blocs de code, listes et
citations en Markdown. La ligne d'en-tête reste visible, suivie de « … » ; la ligne d'une
accolade fermante seule aussi. Un carré dans la gouttière marque un bloc replié ; un clic dessus,
sur le « … » ou dans la gouttière d'un bloc repliable le bascule.

Les curseurs sautent les lignes masquées (↑, ↓, ←, →) ; un curseur placé dans un bloc replié
(recherche, aller à la ligne…) le déplie. Les plis suivent les modifications du texte.

| Commande | Description |
|---|---|
| `fold close [--line N]` | Replie le plus petit bloc qui contient la ligne du curseur (ou la ligne `N`) et n'est pas encore replié : répétée, elle replie les blocs englobants (`Ctrl+Maj+[` ; `Option+Cmd+[`). |
| `fold open [--line N]` | Déplie le bloc replié sur cette ligne, ou qui la contient (`Ctrl+Maj+]` ; `Option+Cmd+]`). |
| `fold toggle [--line N]` | Déplie le bloc replié sur cette ligne, sinon replie le bloc qui la contient (`mod+k mod+l`). |
| `fold close-all` | Replie tous les blocs du document (`mod+k mod+0`). |
| `fold open-all` | Déplie tout (`mod+k mod+j`). |
| `fold list` | Blocs repliés : ligne d'en-tête et lignes masquées. |

## `syntax` — arbre syntaxique

Le langage d'un document est détecté d'après l'extension ou le nom de son fichier, sinon d'après
sa ligne `#!` (`#!/usr/bin/env python3`). Les grammaires Tree-sitter de Rust, C et Markdown sont
intégrées en attendant les paquets de langage. Le code de ces langages est coloré selon les
couleurs `[syntax]` du thème, blocs de code Markdown compris (`` ```rust ``, `` ```c ``).

| Commande | Description |
|---|---|
| `syntax tree [--full]` | Nœuds de l'arbre syntaxique sous le curseur, du plus large au plus précis, avec leur position (`source_file (1:1) › function_item (1:1) › …`) ; `--full` : tout l'arbre, en S-expression. Pour écrire des requêtes ou comprendre une coloration. |

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
| `view wrap [on\|off\|toggle]` | Retour à la ligne visuel pour la session, à la place de l'option `[view] wrap` (`Alt+Z` sous Windows et Linux). |

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

Toutes les fenêtres partagent les mêmes documents ; chacune a son propre layout (zones, onglets).
Les fenêtres sont numérotées à partir de 1, dans l'ordre de leur ouverture ; un numéro ne sert
qu'une fois par session. La ligne de commande, la palette et les notifications s'affichent dans
la fenêtre qui a le focus.

Un document n'existe qu'une fois en mémoire : une modification apparaît aussitôt dans toutes les
fenêtres qui l'affichent, et annuler ou rétablir agit sur ce document où qu'il soit affiché.
Chaque fenêtre garde ses propres curseurs et son défilement, et sa barre d'état décrit son propre
panneau actif. Seules les fenêtres qui affichent ce qui a changé sont redessinées.

| Commande | Description |
|---|---|
| `window new [--monitor N]` | Ouvre une nouvelle fenêtre, avec un document vide (`mod+shift+n`) ; avec `--monitor`, centrée sur l'écran `N`. |
| `window move --monitor N [fenêtre]` | Déplace la fenêtre active (ou la fenêtre donnée) sur l'écran `N` : centrée, réduite à 90 % de l'écran au plus pour y tenir, sortie du plein écran. |
| `window list-monitors` | Liste les écrans : numéro, nom, résolution (pixels physiques), échelle ; l'écran principal est signalé. |
| `window close [N] [--force]` | Ferme la fenêtre active, ou la fenêtre `N` (`mod+shift+w`). Ses documents qui ne sont affichés dans aucune autre fenêtre sont fermés : s'ils ont des modifications non enregistrées, la commande échoue, sauf avec `--force`. Fermer la dernière fenêtre quitte CuprIDE (option `[window] quit_on_last_close`). |
| `window focus <N>` | Donne le focus à la fenêtre `N`. |
| `window list` | Liste les fenêtres : numéro, document actif, nombre de panneaux ; `>` marque la fenêtre active. |

**Numérotation des écrans** : à partir de 1, de gauche à droite, puis de haut en bas, d'après la
position du coin haut gauche de chaque écran sur le bureau. Un même agencement d'écrans donne
toujours les mêmes numéros, quel que soit l'ordre dans lequel l'OS les énumère ; brancher un
écran à gauche décale les numéros des écrans à sa droite. La liste est relue au plus une fois par
seconde.

Le bouton de fermeture d'une fenêtre fait comme `window close` ; avec des modifications non
enregistrées, il faut fermer une seconde fois pour confirmer.

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
| `panel move <direction> [--split]` | Déplace le panneau actif (avec son document, ses curseurs et son défilement) dans la zone voisine de ce côté, en onglet ; s'il n'y en a pas, ou avec `--split`, dans une nouvelle zone de ce côté de la zone active. Une zone vidée disparaît. Raccourcis : `mod+k` puis `alt+←`, `→`, `↑`, `↓`. |
| `panel move --window N` | Envoie le panneau actif dans la fenêtre `N` (voir `window list`), en onglet à côté de son panneau actif, par exemple pour le mettre sur un autre écran. Il garde son document, ses curseurs et son défilement, et le focus le suit. |
| `panel resize <sens>` | Agrandit ou réduit la zone active de 5 % de la fenêtre : `wider`, `narrower` (largeur), `taller`, `shorter` (hauteur). La place est prise à la zone voisine (la suivante, ou la précédente pour la dernière) ; chaque zone garde une taille minimale (sa barre d'onglets et trois lignes). Raccourcis : `mod+k` puis `mod+shift+→`, `←`, `↓`, `↑`. |
| `panel list` | Liste les panneaux, zone par zone : `>` marque le panneau actif. |

Le panneau actif se reconnaît à son onglet, souligné en haut de la couleur d'accent ; les titres
des autres zones sont en gris. Un clic dans une zone lui donne le focus. Les séparations entre
les zones se glissent à la souris (le pointeur change au-dessus), dans la limite de la taille
minimale des zones.

Un onglet se déplace aussi par **glisser-déposer** : lâché sur une barre d'onglets ou au centre
d'une zone, il y devient un onglet ; lâché près d'un bord d'une zone (le quart le plus proche), il
crée une nouvelle zone de ce côté. Pendant le glisser, la place qu'il prendrait est surlignée.

Exemples :

```
panel open explorer --split left
panel open editor --split down
```

## `explorer` — explorateur de fichiers

L'explorateur affiche l'arborescence du projet (le dossier ouvert avec `cupride <dossier>`, sinon
le dossier courant). Chaque dossier n'est lu qu'au moment où on le déplie, et l'arborescence se met
à jour d'elle-même quand le disque change (dossiers affichés surveillés avec `notify`). Les fichiers
ignorés par `.gitignore` et `.ignore` sont en gris, ou masqués (option `[explorer] ignored`) ; le
dossier `.git` n'est jamais affiché.

| Commande | Description |
|---|---|
| `explorer show` | Affiche l'explorateur et lui donne le focus ; s'il n'est pas ouvert, il s'ouvre à gauche, sur une colonne étroite (`mod+shift+e`). |
| `explorer reveal` | Montre le document actif dans l'explorateur : dossiers parents dépliés, fichier sélectionné. |
| `explorer new-file <chemin>` | Crée un fichier vide (jamais par-dessus un fichier existant) et l'ouvre. |
| `explorer new-folder <chemin>` | Crée un dossier. |
| `explorer rename <source> <cible>` | Renomme un fichier ou un dossier, jamais par-dessus un fichier existant ; les documents ouverts suivent leur fichier. |
| `explorer move <source> <cible>` | Déplace un fichier ou un dossier ; une cible qui est un dossier existant le reçoit. Les documents ouverts suivent. |
| `explorer delete <chemin>` | Met un fichier ou un dossier à la corbeille de l'OS. Les documents de ces fichiers sont fermés, sauf ceux qui ont des modifications non enregistrées. |

Un chemin relatif part du dossier du projet. Dans l'explorateur, tout se fait au clavier :

| Touche | Effet |
|---|---|
| `↑`, `↓`, `Page préc.`, `Page suiv.`, `Début`, `Fin` | Déplace la sélection. |
| `→` | Déplie le dossier, ou descend sur son premier enfant. |
| `←` | Replie le dossier, ou remonte sur le dossier parent. |
| `Entrée` | Ouvre le fichier, ou plie et déplie le dossier. |
| `Espace` | Ouvre le fichier dans l'onglet d'aperçu (option `[tabs] preview`). |
| `F2` | `explorer rename`, prérempli dans la ligne de commande. |
| `Suppr` (`Cmd+Retour arrière` sous macOS) | `explorer delete`, prérempli : Entrée confirme. |
| `n`, `Maj+N` | `explorer new-file` ou `new-folder`, dans le dossier sélectionné. |
| `m` | `explorer move`, prérempli. |

Les autres raccourcis (`mod+p`, `mod+shift+p`…) restent actifs. À la souris : un clic plie ou
déplie un dossier, et ouvre un fichier en aperçu ; un double clic l'ouvre dans un onglet gardé.

## `layout` — layouts nommés

Un layout nommé garde une disposition complète — zones et proportions, onglets, fichiers,
curseurs, défilement — pour basculer en une commande entre écriture, débogage et revue. Les
layouts sont enregistrés en JSON dans le dossier de données de l'OS, par projet
(`layouts/<projet>/<nom>.json`).

| Commande | Description |
|---|---|
| `layout save <nom>` | Enregistre la disposition actuelle sous ce nom (remplace un layout du même nom). Nom : lettres, chiffres, espaces, `-` et `_`. Les documents sans fichier n'y sont pas gardés. |
| `layout load <nom> [--force]` | Remplace la disposition actuelle par le layout. Les documents qu'il n'affiche pas sont fermés ; un document modifié ne l'est **jamais** sans confirmation : la commande échoue en le nommant, sauf avec `--force`, qui abandonne ses modifications. Un fichier supprimé depuis : son panneau est fermé, avec une notification. |
| `layout list` | Liste les layouts enregistrés pour ce projet. |
| `layout delete <nom>` | Supprime un layout enregistré. |

Exemples :

```
layout save "revue de code"
layout load écriture
```

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
