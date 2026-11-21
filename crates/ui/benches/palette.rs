//! Palette : recherche floue dans 5 000 commandes, à chaque frappe.
//!
//! Commande : `cargo bench -p cupride-ui --bench palette`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use cupride_ui::{Picker, PickerItem};

/// 5 000 commandes : 50 groupes de 100 commandes, avec une description chacune.
fn items() -> Vec<PickerItem> {
    let verbs = [
        "open", "close", "toggle", "select", "move", "insert", "delete", "show", "run", "find",
    ];
    (0..5_000)
        .map(|i| PickerItem {
            label: format!(
                "groupe{:02} {}-{:02}",
                i / 100,
                verbs[i % 10],
                (i / 10) % 10
            ),
            detail: format!("Description de la commande numéro {i}"),
            hint: String::new(),
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("palette_5000_commandes");
    // Chaque mesure : deux frappes, donc deux recherches complètes (la seconde ajoute une
    // lettre, comme en tapant). Le temps d'une recherche est la moitié.
    let pairs = [
        ("t", "to"),
        ("tog", "togg"),
        ("g42 tog", "g42 togg"),
        ("grp4se", "grp4sel"),
    ];
    for (first, second) in pairs {
        let mut picker = Picker::new("Commande", items(), vec![], false);
        group.bench_function(
            format!("2 recherches « {first} » puis « {second} »"),
            |b| {
                b.iter(|| {
                    picker.input.set_text(first);
                    let a = picker.results().len();
                    picker.input.set_text(second);
                    black_box(a + picker.results().len())
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
