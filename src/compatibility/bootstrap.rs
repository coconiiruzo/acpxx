use super::{CatalogError, CatalogKeyring, VerifiedCatalog};

const OFFICIAL_KEY_2026_01: [u8; 32] = [
    0x32, 0xc3, 0x93, 0x0c, 0xc4, 0xe2, 0xa7, 0x5a, 0x17, 0x35, 0x9e, 0x9b, 0x61, 0x24, 0x6b, 0x1c,
    0xb9, 0xd0, 0xda, 0xa1, 0x07, 0x05, 0x14, 0xa1, 0x0a, 0x0b, 0x73, 0xcc, 0x14, 0x27, 0x7c, 0x82,
];

const BOOTSTRAP_CATALOG: &[u8] = include_bytes!("../../compatibility/bootstrap/catalog-v1.json");
const BOOTSTRAP_SIGNATURE: &[u8] = include_bytes!("../../compatibility/bootstrap/catalog-v1.sig");

pub fn official_keyring() -> Result<CatalogKeyring, CatalogError> {
    let mut keyring = CatalogKeyring::new();
    keyring.insert("agentmux-catalog-2026-01", &OFFICIAL_KEY_2026_01)?;
    Ok(keyring)
}

pub fn bootstrap_catalog() -> Result<VerifiedCatalog, CatalogError> {
    VerifiedCatalog::verify(BOOTSTRAP_CATALOG, BOOTSTRAP_SIGNATURE, &official_keyring()?)
}
