//! AES-128-CBC fallback for protocol-v2 devices. MINI-D uses v3 and
//! bypasses this — kept for completeness. Implemented if/when needed.

#[allow(dead_code)]
pub fn decrypt(_payload: &[u8], _key: &[u8]) -> Vec<u8> {
    unimplemented!("not used on MINI-D (protocol v3)")
}
