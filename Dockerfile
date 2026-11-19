# syntax=docker/dockerfile:1
#
# Images de CuprIDE. CuprIDE est une application de bureau : ces images servent
# à compiler, vérifier et tester, pas à distribuer l'IDE.
#
#   docker compose run --rm dev                                  # shell de développement
#   docker compose run --rm ci                                   # mêmes vérifications que la CI
#   docker build --target dist --output type=local,dest=dist .   # binaire Linux de release dans ./dist
#   docker compose --profile gui up gui                          # lancer l'IDE (Linux + X11 uniquement)
#
# Cibles :
#   base     outils de compilation Rust + bibliothèques système
#   dev      base + outils de qualité, utilisateur non root (code monté depuis l'hôte)
#   builder  binaire de release, avec cache des dépendances (cargo-chef)
#   dist     image vide contenant uniquement le binaire, pour l'export
#   gui      exécution de l'IDE sous Linux (X11), pour tester dans un système vierge

# À aligner avec rust-toolchain.toml.
ARG RUST_VERSION=1.95

# =============================================================================
# 1. Base : toolchain Rust + bibliothèques nécessaires à la compilation
# =============================================================================
FROM rust:${RUST_VERSION}-slim-bookworm AS base

# build-essential : compilateur C (grammaires tree-sitter, crates avec du code C).
# lib*-dev : en-têtes pour le fenêtrage Linux (X11, Wayland, clavier).
# À compléter au fil des crates ajoutées (voir README, « Prérequis »).
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config build-essential git \
    libxkbcommon-dev libwayland-dev \
    libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
    libfontconfig1-dev \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy \
    && cargo install cargo-chef --locked

WORKDIR /workspace

# =============================================================================
# 2. Développement : outils de qualité + utilisateur non root
#    Le code n'est pas copié : il est monté depuis l'hôte par docker compose.
# =============================================================================
FROM base AS dev

RUN cargo install cargo-deny --locked

# Même UID/GID que l'utilisateur de l'hôte (Linux) pour que les fichiers créés
# dans le dépôt ne lui appartiennent pas en root. Sans effet gênant sur macOS.
ARG USER_UID=1000
ARG USER_GID=1000
RUN groupadd --non-unique --gid "${USER_GID}" dev \
    && useradd --non-unique --uid "${USER_UID}" --gid "${USER_GID}" --create-home --shell /bin/bash dev \
    # Dossiers qui recevront des volumes nommés : un volume neuf hérite de ces droits.
    && mkdir -p /usr/local/cargo/registry /usr/local/cargo/git /cargo-target \
    # rustup doit pouvoir installer la version de rust-toolchain.toml si elle diffère.
    && chown -R dev:dev /usr/local/cargo /usr/local/rustup /cargo-target

USER dev
ENV CARGO_TARGET_DIR=/cargo-target
CMD ["bash"]

# =============================================================================
# 3. Plan des dépendances (recipe.json) — change rarement
# =============================================================================
FROM base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# =============================================================================
# 4. Binaire de release : dépendances d'abord (couche en cache tant que
#    recipe.json ne bouge pas), puis le code.
#    Profil release défini dans Cargo.toml : lto, codegen-units = 1, strip.
# =============================================================================
FROM base AS builder
COPY --from=planner /workspace/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin cupride

# =============================================================================
# 5. Export : uniquement le binaire
#    docker build --target dist --output type=local,dest=dist .
# =============================================================================
FROM scratch AS dist
COPY --from=builder /workspace/target/release/cupride /cupride

# =============================================================================
# 6. Exécution graphique sous Linux (X11), pour tester dans un système vierge.
#    Ne fonctionne ni sous macOS ni sous Windows : lancer CuprIDE nativement.
# =============================================================================
FROM debian:bookworm-slim AS gui

# Bibliothèques chargées à l'exécution par le fenêtrage et le GPU,
# pilotes Mesa (Vulkan + OpenGL), et au moins une police.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libxkbcommon0 libxkbcommon-x11-0 \
    libx11-6 libx11-xcb1 libxcursor1 libxrandr2 libxi6 \
    libwayland-client0 libwayland-cursor0 \
    libvulkan1 mesa-vulkan-drivers libgl1 libegl1 \
    fontconfig fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*

ARG USER_UID=1000
ARG USER_GID=1000
RUN groupadd --non-unique --gid "${USER_GID}" app \
    && useradd --non-unique --uid "${USER_UID}" --gid "${USER_GID}" --create-home --shell /usr/sbin/nologin app \
    && mkdir -p /home/app/.config/cupride /home/app/.local/share/cupride /projects \
    && chown -R app:app /home/app /projects

COPY --from=builder /workspace/target/release/cupride /usr/local/bin/cupride

USER app
ENV CUPRIDE_LOG=info
WORKDIR /projects
ENTRYPOINT ["/usr/local/bin/cupride"]
CMD ["/projects"]