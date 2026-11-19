# Contribuer à CuprIDE

## Outillage

- La version de Rust est figée dans [`rust-toolchain.toml`](rust-toolchain.toml) : `rustup`
  l'installe automatiquement au premier `cargo`. Elle doit rester alignée avec `ARG RUST_VERSION`
  du [`Dockerfile`](Dockerfile) et `rust-version` du [`Cargo.toml`](Cargo.toml).
- Le formatage suit [`rustfmt.toml`](rustfmt.toml) (options stables uniquement).
- Les règles de lint communes sont définies dans `[workspace.lints]` du `Cargo.toml` racine ;
  chaque crate les active avec `[lints] workspace = true`.

Avant de pousser, ces commandes doivent passer (ou `docker compose run --rm ci`) :

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

## Organisation du code

- Une crate par responsabilité, toutes préfixées `cupride-` (`cupride-text`, `cupride-core`…).
  Le binaire final s'appelle `cupride`.
- Les dépendances vont **uniquement vers le bas** : `text` et `layout` n'ont aucune dépendance
  interne, `core` ne dépend jamais de `render`, `ui` ou `app`.
- Les versions des dépendances sont déclarées une seule fois, dans `[workspace.dependencies]`,
  et utilisées dans les crates avec `<dep>.workspace = true`.

## Gestion des erreurs

| Où | Outil | Règle |
|---|---|---|
| Crates de bibliothèque (`crates/*`) | [`thiserror`](https://docs.rs/thiserror) | Erreurs **typées** : un `enum` d'erreur par domaine, que l'appelant peut inspecter. Jamais d'`anyhow` dans une API publique. |
| Binaires (`app`, `tools/*`) | [`anyhow`](https://docs.rs/anyhow) | Erreurs pratiques à propager avec `?`, enrichies par `.context(...)`, converties en messages lisibles pour l'utilisateur. |

- Pas de `unwrap()` hors des tests : le lint `clippy::unwrap_used` le signale. Utiliser `?`, ou
  `expect("raison")` quand l'invariant est garanti, avec la raison en clair.
- Aucune erreur d'un plugin, d'un serveur LSP ou d'un fichier illisible ne doit faire planter l'IDE.

## Journalisation

- Les logs passent par [`tracing`](https://docs.rs/tracing) ; jamais de `println!` pour diagnostiquer.
- Le niveau se règle avec la variable `CUPRIDE_LOG` (syntaxe des filtres `tracing-subscriber`,
  par exemple `CUPRIDE_LOG=debug` ou `CUPRIDE_LOG=cupride_core=trace`). Par défaut : `warn`.

## Branches et commits

- `main` : toujours fonctionnel sur les trois OS, protégé (PR et CI verte obligatoires).
- `dev` : branche d'intégration.
- Une branche par épic, créée depuis `dev` (par exemple `infra`), puis une branche par issue
  créée depuis la branche d'épic (par exemple `infra-01`).
- Un commit par issue terminée, avec `Closes #<numéro>` dans le message pour fermer l'issue
  à l'arrivée sur `main`.
