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
| Latence de frappe | imperceptible | à mesurer (phase 3) |

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
