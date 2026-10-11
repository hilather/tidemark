//! Owned by: W4-B
//! firstboot implementation reserved for W4-B.

/// Shared component status payload used by firstboot and the daemon API.
pub use tidemark_core::api::ComponentState;

#[cfg(test)]
mod tests {
    #[test]
    fn component_state_is_the_core_type() {
        assert_eq!(
            std::any::TypeId::of::<super::ComponentState>(),
            std::any::TypeId::of::<tidemark_core::api::ComponentState>(),
        );
    }
}
