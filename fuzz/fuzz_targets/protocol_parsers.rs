#![no_main]

use libfuzzer_sys::fuzz_target;
use sprout_api_contract::{PushSyncRequest, SyncEventDto};
use sprout_crypto_protocol::{
    CanonicalHeader, DevicePublicPackage, EncryptedPayload, ExperimentalWrappedResourceKey,
    RecoveryShare,
};

const MAX_FUZZ_INPUT_BYTES: usize = 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FUZZ_INPUT_BYTES {
        return;
    }

    let _ = CanonicalHeader::from_canonical_bytes(data);
    let _ = EncryptedPayload::from_bytes(data);
    let _ = ExperimentalWrappedResourceKey::from_bytes(data);
    let _ = RecoveryShare::from_bytes(data);
    let _ = DevicePublicPackage::from_json(data);
    let _ = serde_json::from_slice::<PushSyncRequest>(data);
    let _ = serde_json::from_slice::<SyncEventDto>(data);
});
