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

Insertion, suppression, annuler, rétablir, déplacements du curseur et sélection (CMD-04).
Toute action clavier de l'éditeur correspond à l'une de ces commandes.

## `config` — configuration

| Commande | Description | Issue |
|---|---|---|
| `config open` | Ouvre le fichier de configuration dans l'IDE | CORE-04 |

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
