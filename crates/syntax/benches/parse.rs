//! Analyse syntaxique d'un fichier C de 50 000 lignes : première analyse complète, puis frappe
//! (un caractère tapé au milieu du fichier, transmis à Tree-sitter et réanalysé).
//!
//! Commande : `cargo bench -p cupride-syntax --bench parse`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use cupride_syntax::{Syntax, config_for};
use cupride_text::{Edit, Rope, TextBuffer};

/// Fichier C de 50 000 lignes : 5 000 fonctions de 10 lignes.
pub fn c_file() -> String {
    (0..5_000)
        .map(|i| {
            format!(
                "/* Fonction {i} */\n\
                 static int calcul_{i}(const struct donnees *d, int n) {{\n\
                 \x20   int total = 0;\n\
                 \x20   for (int k = 0; k < n; k++) {{\n\
                 \x20       if (d->valeurs[k] > {i}) total += d->valeurs[k] * 2;\n\
                 \x20       else total -= 1;\n\
                 \x20   }}\n\
                 \x20   printf(\"%d\\n\", total);\n\
                 \x20   return total;\n\
                 }}\n"
            )
        })
        .collect()
}

fn parse(c: &mut Criterion) {
    let text = c_file();
    assert_eq!(text.lines().count(), 50_000);
    let rope = Rope::from_text(&text);
    let config = config_for("C").expect("grammaire C");
    let mut group = c.benchmark_group("c_50k");
    group.sample_size(10);
    group.bench_function("analyse_complete", |b| {
        b.iter(|| black_box(Syntax::new(config, &rope, None)));
    });
    group.finish();

    let mut rope = rope;
    let mut syntax = Syntax::new(config, &rope, None);
    // Au milieu du fichier, dans le corps d'une fonction.
    let at = rope.line_to_byte(25_002).expect("ligne") + 8;
    let mut insert = true;
    c.bench_function("c_50k/frappe", |b| {
        b.iter(|| {
            let edit = if insert {
                Edit::insert(at, "x")
            } else {
                Edit::delete(&rope, at..at + 1).expect("suppression")
            };
            insert = !insert;
            edit.apply(&mut rope).expect("modification");
            syntax.edited(&mut rope, &[edit]);
            black_box(syntax.tree());
        });
    });
}

criterion_group!(benches, parse);
criterion_main!(benches);
