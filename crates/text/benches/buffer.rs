//! Benchmarks des opérations critiques du buffer, sur 1 Mo et 100 Mo.
//!
//! `cargo bench -p cupride-text` ; rapports HTML dans `target/criterion/`.
//! Objectif : insertion au milieu d'un fichier de 100 Mo en moins de 1 ms.

// Un benchmark n'a pas d'erreur à gérer : un échec doit interrompre la mesure.
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use cupride_text::{Rope, StringBuffer, TextBuffer};

const MB: usize = 1024 * 1024;
const SIZES: [(usize, &str); 2] = [(MB, "1 Mo"), (100 * MB, "100 Mo")];

/// Lignes visibles à l'écran.
const VISIBLE_LINES: usize = 60;

/// Code source réaliste d'environ `size` octets : ASCII majoritaire, quelques caractères
/// multi-octets.
fn source(size: usize) -> String {
    let line = "    let valeur = calcul(42, \"chaîne\"); // commentaire é😀\n";
    line.repeat(size / line.len() + 1)
}

/// Position d'une frontière de caractère proche du milieu.
fn middle(rope: &Rope) -> usize {
    let mut offset = rope.len_bytes() / 2;
    while !rope.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn insertion(c: &mut Criterion) {
    let mut group = c.benchmark_group("insertion au milieu");
    for (size, label) in SIZES {
        let text = source(size);
        let mut rope = Rope::from_text(&text);
        let mid = middle(&rope);
        group.bench_function(BenchmarkId::new("rope", label), |b| {
            // Seules les insertions sont mesurées ; le texte est remis en état ensuite.
            b.iter_custom(|iters| {
                let start = Instant::now();
                for _ in 0..iters {
                    rope.insert(black_box(mid), black_box("x")).unwrap();
                }
                let elapsed = start.elapsed();
                rope.remove(mid..mid + iters as usize).unwrap();
                elapsed
            });
        });
        // Référence naïve : chaque insertion décale toute la fin du texte.
        let mut naive = StringBuffer::from_text(&text);
        group.bench_function(BenchmarkId::new("string (référence)", label), |b| {
            b.iter_custom(|iters| {
                let start = Instant::now();
                for _ in 0..iters {
                    naive.insert(black_box(mid), black_box("x")).unwrap();
                }
                let elapsed = start.elapsed();
                naive.remove(mid..mid + iters as usize).unwrap();
                elapsed
            });
        });
    }
    group.finish();
}

fn suppression(c: &mut Criterion) {
    let mut group = c.benchmark_group("suppression au milieu");
    for (size, label) in SIZES {
        let mut rope = Rope::from_text(&source(size));
        let mid = middle(&rope);
        group.bench_function(BenchmarkId::new("rope", label), |b| {
            // Le texte à supprimer est inséré d'abord, hors mesure.
            b.iter_custom(|iters| {
                rope.insert(mid, &"x".repeat(iters as usize)).unwrap();
                let start = Instant::now();
                for _ in 0..iters {
                    rope.remove(black_box(mid..mid + 1)).unwrap();
                }
                start.elapsed()
            });
        });
    }
    group.finish();
}

fn conversions(c: &mut Criterion) {
    let mut group = c.benchmark_group("conversion position ↔ ligne");
    for (size, label) in SIZES {
        let rope = Rope::from_text(&source(size));
        let mid = middle(&rope);
        let line = rope.byte_to_line(mid).unwrap();
        group.bench_function(BenchmarkId::new("position → ligne", label), |b| {
            b.iter(|| rope.byte_to_line(black_box(mid)).unwrap());
        });
        group.bench_function(BenchmarkId::new("ligne → position", label), |b| {
            b.iter(|| rope.line_to_byte(black_box(line)).unwrap());
        });
    }
    group.finish();
}

fn lignes_visibles(c: &mut Criterion) {
    let mut group = c.benchmark_group("lignes visibles");
    for (size, label) in SIZES {
        let rope = Rope::from_text(&source(size));
        let first = rope.byte_to_line(middle(&rope)).unwrap();
        group.bench_function(
            BenchmarkId::new(format!("{VISIBLE_LINES} lignes"), label),
            |b| {
                b.iter(|| {
                    let mut total = 0;
                    for line in first..first + VISIBLE_LINES {
                        total += rope.line(black_box(line)).unwrap().len();
                    }
                    total
                });
            },
        );
    }
    group.finish();
}

fn chargement(c: &mut Criterion) {
    let mut group = c.benchmark_group("chargement");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));
    for (size, label) in SIZES {
        let text = source(size);
        group.bench_function(BenchmarkId::new("Rope::from_text", label), |b| {
            b.iter(|| Rope::from_text(black_box(&text)));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    insertion,
    suppression,
    conversions,
    lignes_visibles,
    chargement
);
criterion_main!(benches);
