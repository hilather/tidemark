//! Owned by: W2-A
//! Iterative parent-lifting lowest common ancestors.
use super::{AggError, Node, NONE};
pub(super) fn lca(nodes: &[Option<Box<Node>>], mut a: u32, mut b: u32) -> Result<u32, AggError> {
    while a != b {
        let na = nodes
            .get(a as usize)
            .and_then(Option::as_ref)
            .ok_or(AggError::Tree)?;
        let nb = nodes
            .get(b as usize)
            .and_then(Option::as_ref)
            .ok_or(AggError::Tree)?;
        if na.depth >= nb.depth {
            a = na.parent;
        }
        if nb.depth >= na.depth {
            b = nb.parent;
        }
        if a == NONE || b == NONE {
            return Err(AggError::Tree);
        }
    }
    Ok(a)
}
