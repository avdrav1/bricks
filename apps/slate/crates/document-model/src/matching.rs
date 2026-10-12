//! Matching a document's blocks against the blocks its file was read with, so a save
//! copies the source of the blocks that didn't change (Markdown, DOCX).

use crate::Block;

/// The largest unmatched middle (old blocks × new blocks) matched block by block.
const MATCH_LIMIT: usize = 4_000 * 4_000;

/// For each new block, the old block it is (`None`: changed or new): equal blocks matched
/// in order. The common start and end match first, then the longest common subsequence of
/// equal blocks in between; a middle over 16 million block pairs counts as changed.
pub fn match_blocks(old: &[Block], new: &[Block]) -> Vec<Option<usize>> {
    match_blocks_by(old, new, |a, b| a == b)
}

/// [`match_blocks`], with `same` telling which blocks are equal.
pub fn match_blocks_by(
    old: &[Block],
    new: &[Block],
    same: impl Fn(&Block, &Block) -> bool,
) -> Vec<Option<usize>> {
    let mut kept = vec![None; new.len()];
    let prefix = old.iter().zip(new).take_while(|(a, b)| same(a, b)).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(room)
        .take_while(|(a, b)| same(a, b))
        .count();
    for (k, slot) in kept.iter_mut().enumerate().take(prefix) {
        *slot = Some(k);
    }
    for k in 0..suffix {
        kept[new.len() - 1 - k] = Some(old.len() - 1 - k);
    }
    let a = &old[prefix..old.len() - suffix];
    let b = &new[prefix..new.len() - suffix];
    if !a.is_empty() && !b.is_empty() && a.len().saturating_mul(b.len()) <= MATCH_LIMIT {
        for (i, j) in common_subsequence(a, b, &same) {
            kept[prefix + j] = Some(prefix + i);
        }
    }
    kept
}

/// The longest common subsequence of equal blocks, as index pairs in order.
fn common_subsequence(
    a: &[Block],
    b: &[Block],
    same: &impl Fn(&Block, &Block) -> bool,
) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let width = m + 1;
    // lcs[i][j]: the longest common subsequence of a[i..] and b[j..] (≤ 4,000: one side of
    // a middle within the limit is at most its square root).
    let mut lcs = vec![0u16; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * width + j] = if same(&a[i], &b[j]) {
                lcs[(i + 1) * width + j + 1] + 1
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let mut pairs = Vec::with_capacity(lcs[0] as usize);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same(&a[i], &b[j]) {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * width + j] >= lcs[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}
