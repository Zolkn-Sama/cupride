# Mesures de performance

Historique des mesures des opérations critiques. Chaque série indique la date, le commit, la
machine et la commande, pour pouvoir comparer d'une série à l'autre.

Les mesures en conteneur ou en CI servent à détecter des régressions relatives ; les objectifs se
vérifient en natif, en mode release.

## Objectifs

| Mesure | Objectif | Statut |
|---|---|---|
| Insertion au milieu d'un fichier de 100 Mo | < 1 ms | ✅ 0,72 µs (2026-11-19) |
| Démarrage | < 100 ms | à mesurer (phase 3) |
| Mémoire au repos | < 50 Mo | ❌ 85 Mo, fenêtre vide sans GPU (2026-11-19) |
| CPU au repos | 0 % | ✅ 0,00 s de CPU en 20 s, fenêtre vide (2026-11-19) |
| Latence de frappe | imperceptible | ✅ 0,66 ms de la touche à l'image rendue, hors synchronisation de l'écran (2026-11-21) |

## Buffer de texte (`cupride-text`)

Commande : `cargo bench -p cupride-text` (rapports HTML dans `target/criterion/`).

Texte mesuré : du code source réaliste, en lignes de ~60 octets mêlant ASCII et caractères
multi-octets (`é`, `😀`), répété jusqu'à 1 Mo ou 100 Mo. Valeurs : médiane de criterion.

### 2026-11-19 — TEXT-08, première série

- Machine : Apple M3 Pro, 18 Go, macOS ; Rust 1.95.0, profil `bench` (release)
- Rope : feuilles de 1 à 4 Ko, 8 à 16 enfants par nœud

| Opération | 1 Mo | 100 Mo |
|---|---|---|
| Insertion d'un caractère au milieu — rope | 0,65 µs | **0,72 µs** |
| Insertion d'un caractère au milieu — `String` (référence) | 8,6 µs | 938 µs |
| Suppression d'un caractère au milieu — rope | 0,44 µs | 0,52 µs |
| Conversion position → ligne | 0,27 µs | 0,53 µs |
| Conversion ligne → position | 0,38 µs | 0,74 µs |
| Lecture de 60 lignes visibles (une par une) | 48,8 µs | 45,6 µs |
| Chargement (`Rope::from_text`) | 0,18 ms | 21,6 ms |

Lecture :

- Les modifications et les conversions restent sous la microseconde à 100 Mo, et ne coûtent
  qu'environ 10 à 100 % de plus qu'à 1 Mo : le coût est bien logarithmique. La `String` de
  référence est 1 300 fois plus lente à 100 Mo, parce qu'elle décale toute la fin du texte.
- La lecture des lignes visibles (~0,8 µs par ligne) alloue une `String` par ligne et refait la
  descente dans l'arbre pour chacune. C'est largement suffisant pour l'affichage (60 lignes en
  moins de 0,05 ms) ; un itérateur de lignes à partir d'une position pourra l'améliorer si le rendu
  le demande.

## Application (`cupride`)

Mesures sur le binaire de release (`cargo build --release`), lancé sans interaction : temps CPU
cumulé (`ps -o time`) et mémoire résidente (`ps -o rss`), relevés après le démarrage.

### 2026-11-19 — RENDER-01, fenêtre vide

- Machine : Apple M3 Pro, 18 Go, macOS ; Rust 1.95.0
- Fenêtre `winit` vide, boucle en attente d'événements (`ControlFlow::Wait`), sans GPU

| Mesure | Valeur |
|---|---|
| Temps CPU consommé en 20 s au repos | 0,00 s (résolution 0,01 s) |
| Mémoire résidente | 85 Mo |

Lecture : la boucle dort réellement au repos. La mémoire dépasse déjà l'objectif de 50 Mo avant
tout rendu : l'essentiel vient des bibliothèques système chargées par une application macOS
fenêtrée (AppKit). L'objectif sera réévalué une fois le rendu en place, en distinguant la mémoire
propre à CuprIDE (`footprint`, `heaptrack`) de celle des bibliothèques partagées.

### 2026-11-21 — RENDER-04, clignotement du curseur

- Machine : Apple M3 Pro, 18 Go, macOS ; Rust 1.95.0, binaire de release
- Fichier de 450 lignes ouvert, fenêtre au premier plan (focus clavier), curseur clignotant
  toutes les 530 ms, sans aucune saisie

| Phase | Temps CPU | Soit |
|---|---|---|
| Clignotement (de 1 s à 10 s, ~17 redessins) | 0,06 s en 9 s | ~0,7 % d'un cœur, ~3,5 ms par redessin |
| Après l'arrêt automatique (de 10 s à 30 s) | 0,01 s en 20 s | le dernier redessin, puis 0 |
| Mémoire résidente | 100 Mo | |

Lecture : le clignotement coûte un redessin complet de la fenêtre par changement d'état ; c'est
peu, mais pas nul. C'est pourquoi il s'arrête, curseur visible, 10 s après la dernière saisie :
l'objectif de 0 % de CPU au repos est tenu. Désactivé (`cursor_blink = false`), aucun réveil
n'est programmé. Le coût d'un redessin (préparation des glyphes, envoi au GPU, présentation)
pourra être réduit en ne redessinant que la zone du curseur si le besoin s'en fait sentir.

## Interface (`cupride-ui`)

Commande : `cargo bench -p cupride-ui`. Vue de la taille de la fenêtre par défaut sur un écran
Retina (2 200 × 1 440 pixels physiques, police 14 pt, ~37 lignes visibles), dans un fichier de
code de 100 000 lignes (ASCII, accents et emojis). Budget d'une image à 60 Hz : 16,7 ms.

### 2026-11-21 — RENDER-05, défilement

- Machine : Apple M3 Pro, 18 Go, macOS ; Rust 1.95.0, profil `bench` (release)

| Image | Temps (médiane) | Part du budget à 60 Hz |
|---|---|---|
| Défilement de 3 lignes : mise en forme des lignes nouvelles et liste de dessin | 0,34 ms | 2 % |
| Saut d'une page : les ~37 lignes visibles mises en forme | 3,4 ms | 20 % |
| Défilement de 3 lignes, image complète avec rendu GPU (attente de la fin du rendu) | 0,76 ms | 5 % |

Lecture : seules les lignes visibles sont lues et mises en forme ; une ligne déjà mise en forme
est réutilisée. Le coût d'une image ne dépend donc pas de la taille du fichier, mais du nombre
de lignes nouvelles à l'écran (~90 µs par ligne mise en forme). Même le pire cas, une page
entièrement nouvelle, laisse 80 % du budget d'une image.

### 2026-11-21 — RENDER-06, latence de frappe

- Machine : Apple M3 Pro, 18 Go, macOS ; Rust 1.95.0, profil `bench` (release)
- Commande : `cargo bench -p cupride-ui --bench typing`
- Fichier de 10 000 lignes de code, vue Retina 2 200 × 1 440 ; chaque mesure est une frappe,
  alternativement un caractère (`edit insert`) et un retour arrière (`edit delete-backward`)

| De la touche à… | Temps (médiane) |
|---|---|
| …la liste de dessin : commande, modification du document, mise en forme de la ligne | 0,21 ms |
| …l'image rendue par le GPU (attente de la fin du rendu) | 0,66 ms |

Lecture : le traitement d'une frappe tient en moins d'un millième de seconde. La latence perçue
est dominée par la synchronisation verticale (jusqu'à 16,7 ms à 60 Hz, la présentation attend le
prochain rafraîchissement) et par l'écran lui-même. L'application mesure la latence réelle, de
l'événement clavier à la présentation de l'image : chaque frappe au niveau `debug`, et un résumé
(nombre, médiane, 99e centile, maximum) à la fermeture, au niveau `info` :
`CUPRIDE_LOG=cupride=debug cupride fichier.rs`.
