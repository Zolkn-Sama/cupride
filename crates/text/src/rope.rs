//! Rope : texte stocké dans un arbre équilibré de morceaux.
//!
//! L'arbre est un B-arbre : toutes les feuilles sont à la même profondeur, chaque feuille contient
//! un morceau de texte de 1 à 4 Ko, et chaque nœud interne a de 8 à 16 enfants (la racine peut en
//! avoir moins). Chaque nœud mémorise le nombre d'octets et de sauts de ligne de son sous-arbre :
//! insérer, supprimer et convertir une position en ligne coûtent O(log n), quelle que soit la
//! taille du texte.
//!
//! Les morceaux sont des `String` : une feuille ne peut donc jamais couper un caractère UTF-8.

use std::fmt;
use std::ops::Range;

use crate::buffer::TextBuffer;
use crate::error::{Result, TextError};

// Tailles réelles. Les tests unitaires utilisent des tailles minuscules pour obtenir des arbres
// profonds avec peu de texte ; les tests d'intégration (`tests/`) utilisent les tailles réelles.
#[cfg(not(test))]
const MAX_LEAF: usize = 4096;
#[cfg(not(test))]
const MIN_LEAF: usize = 1024;
#[cfg(not(test))]
const MAX_CHILDREN: usize = 16;

#[cfg(test)]
const MAX_LEAF: usize = 32;
#[cfg(test)]
const MIN_LEAF: usize = 8;
#[cfg(test)]
const MAX_CHILDREN: usize = 4;

const MIN_CHILDREN: usize = MAX_CHILDREN / 2;

/// Marge sous `MAX_LEAF` lors d'un découpage : reculer une coupure jusqu'à une frontière de
/// caractère (au plus 3 octets) ne doit jamais faire dépasser la taille maximale.
const SPLIT_TARGET: usize = MAX_LEAF - 4;

/// Résumé d'un sous-arbre.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Info {
    bytes: usize,
    newlines: usize,
}

impl Info {
    fn of(text: &str) -> Self {
        Self {
            bytes: text.len(),
            newlines: count_newlines(text),
        }
    }

    fn sum(nodes: &[Node]) -> Self {
        nodes.iter().fold(Self::default(), |acc, node| Self {
            bytes: acc.bytes + node.info.bytes,
            newlines: acc.newlines + node.info.newlines,
        })
    }
}

fn count_newlines(text: &str) -> usize {
    text.bytes().filter(|&b| b == b'\n').count()
}

#[derive(Debug, Clone)]
struct Node {
    info: Info,
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    Leaf(String),
    Internal(Vec<Node>),
}

impl Node {
    fn leaf(text: String) -> Self {
        Self {
            info: Info::of(&text),
            kind: Kind::Leaf(text),
        }
    }

    fn internal(children: Vec<Node>) -> Self {
        Self {
            info: Info::sum(&children),
            kind: Kind::Internal(children),
        }
    }

    fn empty() -> Self {
        Self::leaf(String::new())
    }

    fn is_underfull(&self) -> bool {
        match &self.kind {
            Kind::Leaf(text) => text.len() < MIN_LEAF,
            Kind::Internal(children) => children.len() < MIN_CHILDREN,
        }
    }

    /// Insère `text` à la position locale `offset`. Si le nœud déborde, il est découpé : il garde
    /// la première partie et renvoie les suivantes, à placer juste après lui dans son parent.
    fn insert(&mut self, offset: usize, text: &str) -> Vec<Node> {
        let kind = std::mem::replace(&mut self.kind, Kind::Leaf(String::new()));
        let mut parts = match kind {
            Kind::Leaf(mut leaf) => {
                leaf.insert_str(offset, text);
                split_text(leaf)
                    .into_iter()
                    .map(Node::leaf)
                    .collect::<Vec<_>>()
            }
            Kind::Internal(mut children) => {
                let (index, local) = child_containing(&children, offset);
                let extra = children[index].insert(local, text);
                children.splice(index + 1..index + 1, extra);
                split_evenly(children, MAX_CHILDREN)
                    .into_iter()
                    .map(Node::internal)
                    .collect()
            }
        };
        let rest = parts.split_off(1);
        *self = parts.pop().unwrap_or_else(Node::empty);
        rest
    }

    /// Supprime la plage locale `start..end` (non vide). Les enfants trop petits sont fusionnés
    /// avec leurs voisins ; le nœud lui-même peut devenir trop petit, c'est au parent de le
    /// rééquilibrer.
    fn remove(&mut self, start: usize, end: usize) {
        match &mut self.kind {
            Kind::Leaf(leaf) => leaf.replace_range(start..end, ""),
            Kind::Internal(children) => {
                let mut kept = Vec::with_capacity(children.len());
                let mut child_start = 0;
                for mut child in children.drain(..) {
                    let child_end = child_start + child.info.bytes;
                    if child_end <= start || child_start >= end {
                        kept.push(child);
                    } else if start > child_start || end < child_end {
                        child.remove(
                            start.max(child_start) - child_start,
                            end.min(child_end) - child_start,
                        );
                        kept.push(child);
                    }
                    // Sinon l'enfant est entièrement couvert par la plage : il disparaît.
                    child_start = child_end;
                }
                fix_children(&mut kept);
                *children = kept;
            }
        }
        self.update_info();
    }

    fn update_info(&mut self) {
        self.info = match &self.kind {
            Kind::Leaf(text) => Info::of(text),
            Kind::Internal(children) => Info::sum(children),
        };
    }

    fn collect(&self, start: usize, end: usize, out: &mut String) {
        match &self.kind {
            Kind::Leaf(text) => out.push_str(&text[start..end]),
            Kind::Internal(children) => {
                let mut child_start = 0;
                for child in children {
                    let child_end = child_start + child.info.bytes;
                    if child_end > start && child_start < end {
                        child.collect(
                            start.max(child_start) - child_start,
                            end.min(child_end) - child_start,
                            out,
                        );
                    }
                    if child_end >= end {
                        break;
                    }
                    child_start = child_end;
                }
            }
        }
    }
}

/// Enfant contenant la position `offset`, et la position locale dans cet enfant. Une position
/// située entre deux enfants est attribuée au premier (à sa fin).
fn child_containing(children: &[Node], mut offset: usize) -> (usize, usize) {
    let last = children.len().saturating_sub(1);
    for (index, child) in children.iter().enumerate() {
        if offset <= child.info.bytes || index == last {
            return (index, offset);
        }
        offset -= child.info.bytes;
    }
    (last, offset)
}

/// Découpe un texte trop long en morceaux de taille égale, chacun entre `MIN_LEAF` et `MAX_LEAF`,
/// coupés sur des frontières de caractère. Un texte assez court reste en un seul morceau.
fn split_text(text: String) -> Vec<String> {
    if text.len() <= MAX_LEAF {
        return vec![text];
    }
    let count = text.len().div_ceil(SPLIT_TARGET);
    let mut cuts = Vec::with_capacity(count + 1);
    cuts.push(0);
    for i in 1..count {
        let mut cut = text.len() * i / count;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        cuts.push(cut);
    }
    cuts.push(text.len());
    cuts.windows(2)
        .map(|w| text[w[0]..w[1]].to_owned())
        .collect()
}

/// Répartit `items` en groupes de taille égale d'au plus `max` éléments (au moins `max / 2`
/// chacun s'il y a plusieurs groupes).
fn split_evenly<T>(mut items: Vec<T>, max: usize) -> Vec<Vec<T>> {
    let count = items.len().div_ceil(max).max(1);
    let mut groups = Vec::with_capacity(count);
    for i in (0..count).rev() {
        let size = items.len() / (i + 1);
        let rest = items.split_off(items.len() - size);
        groups.push(rest);
    }
    groups.reverse();
    groups
}

/// Construit l'arbre au-dessus d'une liste de nœuds de même hauteur.
fn build(mut nodes: Vec<Node>) -> Node {
    while nodes.len() > 1 {
        nodes = split_evenly(nodes, MAX_CHILDREN)
            .into_iter()
            .map(Node::internal)
            .collect();
    }
    nodes.pop().unwrap_or_else(Node::empty)
}

/// Retire les enfants vides et fusionne chaque enfant trop petit avec un voisin.
fn fix_children(children: &mut Vec<Node>) {
    children.retain(|child| child.info.bytes > 0);
    let mut i = 0;
    while i < children.len() {
        if children.len() > 1 && children[i].is_underfull() {
            let left = if i + 1 < children.len() { i } else { i - 1 };
            let right_node = children.remove(left + 1);
            let left_node = children.remove(left);
            let merged = merge(left_node, right_node);
            children.splice(left..left, merged);
            i = left;
        } else {
            i += 1;
        }
    }
}

/// Fusionne deux nœuds voisins de même hauteur ; le résultat est redécoupé s'il déborde.
fn merge(left: Node, right: Node) -> Vec<Node> {
    match (left.kind, right.kind) {
        (Kind::Leaf(mut a), Kind::Leaf(b)) => {
            a.push_str(&b);
            split_text(a).into_iter().map(Node::leaf).collect()
        }
        (Kind::Internal(mut a), Kind::Internal(b)) => {
            a.extend(b);
            fix_children(&mut a);
            split_evenly(a, MAX_CHILDREN)
                .into_iter()
                .map(Node::internal)
                .collect()
        }
        // Les voisins d'un B-arbre ont toujours la même hauteur.
        _ => unreachable!("fusion de nœuds de hauteurs différentes"),
    }
}

/// Texte stocké dans un arbre équilibré de morceaux.
///
/// L'arbre est un B-arbre : toutes les feuilles sont à la même profondeur, chaque feuille contient
/// un morceau de texte de 1 à 4 Ko, et chaque nœud interne a de 8 à 16 enfants (la racine peut en
/// avoir moins). Chaque nœud mémorise le nombre d'octets et de sauts de ligne de son sous-arbre :
/// insérer, supprimer et convertir une position en ligne coûtent O(log n), quelle que soit la
/// taille du texte. Les morceaux sont des `String` : une feuille ne coupe jamais un caractère.
#[derive(Debug, Clone)]
pub struct Rope {
    root: Node,
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
    }
}

impl Rope {
    /// Crée une rope vide.
    pub fn new() -> Self {
        Self {
            root: Node::empty(),
        }
    }

    /// Indique si `offset` tombe sur une frontière de caractère (ou au-delà de la fin : faux).
    pub fn is_char_boundary(&self, mut offset: usize) -> bool {
        if offset > self.root.info.bytes {
            return false;
        }
        let mut node = &self.root;
        loop {
            match &node.kind {
                Kind::Leaf(text) => return text.is_char_boundary(offset),
                Kind::Internal(children) => {
                    let (index, local) = child_containing(children, offset);
                    node = &children[index];
                    offset = local;
                }
            }
        }
    }

    /// Hauteur de l'arbre : 0 pour une seule feuille.
    pub fn height(&self) -> usize {
        let mut height = 0;
        let mut node = &self.root;
        while let Kind::Internal(children) = &node.kind {
            height += 1;
            match children.first() {
                Some(child) => node = child,
                None => break,
            }
        }
        height
    }

    /// Vérifie les invariants de l'arbre : résumés exacts, feuilles toutes à la même profondeur,
    /// tailles des feuilles et nombre d'enfants dans les bornes. Réservé aux tests.
    #[doc(hidden)]
    pub fn check_invariants(&self) -> std::result::Result<(), String> {
        fn check(
            node: &Node,
            is_root: bool,
            depth: usize,
            leaf_depth: &mut Option<usize>,
        ) -> std::result::Result<(), String> {
            match &node.kind {
                Kind::Leaf(text) => {
                    if node.info != Info::of(text) {
                        return Err(format!("résumé de feuille faux : {:?}", node.info));
                    }
                    if text.len() > MAX_LEAF {
                        return Err(format!("feuille trop grande : {} octets", text.len()));
                    }
                    if !is_root && text.len() < MIN_LEAF {
                        return Err(format!("feuille trop petite : {} octets", text.len()));
                    }
                    match *leaf_depth {
                        None => *leaf_depth = Some(depth),
                        Some(d) if d != depth => {
                            return Err(format!("feuilles à des profondeurs {d} et {depth}"));
                        }
                        Some(_) => {}
                    }
                }
                Kind::Internal(children) => {
                    if node.info != Info::sum(children) {
                        return Err(format!("résumé de nœud faux : {:?}", node.info));
                    }
                    let min = if is_root { 2 } else { MIN_CHILDREN };
                    if children.len() < min || children.len() > MAX_CHILDREN {
                        return Err(format!("nœud avec {} enfants", children.len()));
                    }
                    for child in children {
                        check(child, false, depth + 1, leaf_depth)?;
                    }
                }
            }
            Ok(())
        }
        check(&self.root, true, 0, &mut None)
    }

    fn check_offset(&self, offset: usize) -> Result<()> {
        if offset > self.len_bytes() {
            return Err(TextError::OffsetOutOfBounds {
                offset,
                len: self.len_bytes(),
            });
        }
        if !self.is_char_boundary(offset) {
            return Err(TextError::NotCharBoundary { offset });
        }
        Ok(())
    }

    fn check_range(&self, range: &Range<usize>) -> Result<()> {
        if range.start > range.end || range.end > self.len_bytes() {
            return Err(TextError::InvalidRange {
                start: range.start,
                end: range.end,
                len: self.len_bytes(),
            });
        }
        for offset in [range.start, range.end] {
            if !self.is_char_boundary(offset) {
                return Err(TextError::NotCharBoundary { offset });
            }
        }
        Ok(())
    }
}

impl TextBuffer for Rope {
    fn from_text(text: &str) -> Self {
        if text.len() <= MAX_LEAF {
            return Self {
                root: Node::leaf(text.to_owned()),
            };
        }
        let leaves = split_text(text.to_owned())
            .into_iter()
            .map(Node::leaf)
            .collect();
        Self {
            root: build(leaves),
        }
    }

    fn len_bytes(&self) -> usize {
        self.root.info.bytes
    }

    fn len_lines(&self) -> usize {
        self.root.info.newlines + 1
    }

    fn insert(&mut self, offset: usize, text: &str) -> Result<()> {
        self.check_offset(offset)?;
        if text.is_empty() {
            return Ok(());
        }
        let extra = self.root.insert(offset, text);
        if !extra.is_empty() {
            let old_root = std::mem::replace(&mut self.root, Node::empty());
            let mut nodes = Vec::with_capacity(extra.len() + 1);
            nodes.push(old_root);
            nodes.extend(extra);
            self.root = build(nodes);
        }
        Ok(())
    }

    fn remove(&mut self, range: Range<usize>) -> Result<()> {
        self.check_range(&range)?;
        if range.is_empty() {
            return Ok(());
        }
        self.root.remove(range.start, range.end);
        // Une racine interne avec un seul enfant est remplacée par cet enfant.
        loop {
            match &mut self.root.kind {
                Kind::Internal(children) if children.len() == 1 => {
                    self.root = children.pop().unwrap_or_else(Node::empty);
                }
                Kind::Internal(children) if children.is_empty() => self.root = Node::empty(),
                _ => break,
            }
        }
        Ok(())
    }

    fn slice(&self, range: Range<usize>) -> Result<String> {
        self.check_range(&range)?;
        let mut out = String::with_capacity(range.len());
        if !range.is_empty() {
            self.root.collect(range.start, range.end, &mut out);
        }
        Ok(out)
    }

    fn byte_to_line(&self, offset: usize) -> Result<usize> {
        self.check_offset(offset)?;
        let mut node = &self.root;
        let mut local = offset;
        let mut line = 0;
        loop {
            match &node.kind {
                Kind::Leaf(text) => return Ok(line + count_newlines(&text[..local])),
                Kind::Internal(children) => {
                    let (index, child_local) = child_containing(children, local);
                    line += children[..index]
                        .iter()
                        .map(|c| c.info.newlines)
                        .sum::<usize>();
                    node = &children[index];
                    local = child_local;
                }
            }
        }
    }

    fn line_to_byte(&self, line: usize) -> Result<usize> {
        if line == 0 {
            return Ok(0);
        }
        if line > self.root.info.newlines {
            return Err(TextError::LineOutOfBounds {
                line,
                count: self.len_lines(),
            });
        }
        // Cherche le `line`-ième saut de ligne : la ligne commence juste après.
        let mut node = &self.root;
        let mut remaining = line;
        let mut offset = 0;
        loop {
            match &node.kind {
                Kind::Leaf(text) => {
                    let pos = text
                        .match_indices('\n')
                        .nth(remaining - 1)
                        .map(|(pos, _)| pos)
                        .ok_or(TextError::LineOutOfBounds {
                            line,
                            count: self.len_lines(),
                        })?;
                    return Ok(offset + pos + 1);
                }
                Kind::Internal(children) => {
                    let mut next = None;
                    for child in children {
                        if remaining <= child.info.newlines {
                            next = Some(child);
                            break;
                        }
                        remaining -= child.info.newlines;
                        offset += child.info.bytes;
                    }
                    node = next.ok_or(TextError::LineOutOfBounds {
                        line,
                        count: self.len_lines(),
                    })?;
                }
            }
        }
    }

    fn chunks(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        Box::new(Chunks {
            stack: vec![&self.root],
        })
    }
}

impl From<&str> for Rope {
    fn from(text: &str) -> Self {
        Self::from_text(text)
    }
}

impl fmt::Display for Rope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.chunks().try_for_each(|chunk| f.write_str(chunk))
    }
}

/// Parcours des feuilles dans l'ordre, en profondeur.
struct Chunks<'a> {
    stack: Vec<&'a Node>,
}

impl<'a> Iterator for Chunks<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        while let Some(node) = self.stack.pop() {
            match &node.kind {
                Kind::Leaf(text) if !text.is_empty() => return Some(text),
                Kind::Leaf(_) => {}
                Kind::Internal(children) => self.stack.extend(children.iter().rev()),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suite::Lcg;

    crate::suite::buffer_suite!(Rope);

    #[test]
    fn creation_equilibree() {
        for len in [
            0,
            1,
            MAX_LEAF,
            MAX_LEAF + 1,
            10 * MAX_LEAF,
            1_000 * MAX_LEAF,
        ] {
            let text = "x".repeat(len);
            let rope = Rope::from_text(&text);
            rope.check_invariants().unwrap();
            assert_eq!(rope.to_text(), text);
        }
        assert_eq!(Rope::from_text("abc").height(), 0);
        assert!(Rope::from_text(&"x".repeat(1_000 * MAX_LEAF)).height() >= 3);
    }

    #[test]
    fn invariants_apres_modifications() {
        let mut rng = Lcg::new(7);
        let mut expected = String::new();
        let mut rope = Rope::new();
        for step in 0..3_000 {
            if rng.below(3) > 0 || expected.is_empty() {
                let offset = rng.boundary(&expected);
                let text = rng.text(if step % 50 == 0 { 2_000 } else { 40 });
                expected.insert_str(offset, &text);
                rope.insert(offset, &text).unwrap();
            } else {
                let a = rng.boundary(&expected);
                let b = rng.boundary(&expected);
                let range = a.min(b)..a.max(b);
                expected.replace_range(range.clone(), "");
                rope.remove(range).unwrap();
            }
            rope.check_invariants()
                .unwrap_or_else(|e| panic!("étape {step} : {e}"));
            assert_eq!(rope.len_bytes(), expected.len());
        }
        assert_eq!(rope.to_text(), expected);
    }

    #[test]
    fn grosse_insertion_puis_suppression_complete() {
        let mut rope = Rope::from_text("début fin");
        let big = "é😀\n".repeat(5_000);
        rope.insert(6, &big).unwrap();
        rope.check_invariants().unwrap();
        assert_eq!(rope.len_lines(), 5_001);
        rope.remove(6..6 + big.len()).unwrap();
        rope.check_invariants().unwrap();
        assert_eq!(rope.to_text(), "début fin");
        assert_eq!(rope.height(), 0);
        rope.remove(0..rope.len_bytes()).unwrap();
        assert!(rope.is_empty());
        rope.check_invariants().unwrap();
    }

    #[test]
    fn decoupage_sur_frontiere_de_caractere() {
        let text = "😀".repeat(MAX_LEAF);
        for piece in split_text(text.clone()) {
            assert!(piece.len() <= MAX_LEAF);
            assert!(piece.len() >= MIN_LEAF);
        }
        assert_eq!(split_text(text.clone()).concat(), text);
    }

    #[test]
    fn affichage() {
        let rope = Rope::from("a\nb");
        assert_eq!(rope.to_string(), "a\nb");
    }
}
