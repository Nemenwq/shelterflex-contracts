//! Ledger helpers for writing TTL tests against mainnet's archival settings.
//!
//! Enabled by the `testutils` feature, the way `soroban-sdk` gates its own test
//! helpers. Contracts pull it in as a dev-dependency:
//!
//! ```toml
//! [dev-dependencies]
//! soroban_storage_ttl = { path = "../soroban_storage_ttl", features = ["testutils"] }
//! ```
//!
//! A TTL test that never advances the ledger proves nothing, and the SDK's
//! default `Env` is far more forgiving than mainnet (a 4096-ledger minimum
//! persistent lifetime against mainnet's 2_073_600, and a much larger cap), so
//! tests must opt into the real numbers with
//! [`set_mainnet_archival_settings`].

pub use crate::LEDGERS_PER_DAY;
use soroban_sdk::{
    testutils::{Ledger, LedgerInfo},
    Env,
};

/// Seconds between ledger closes on mainnet — the basis for [`LEDGERS_PER_DAY`].
pub const SECONDS_PER_LEDGER: u64 = 5;

/// Mainnet's minimum persistent/instance entry lifetime: ~120 days.
pub const MAINNET_MIN_PERSISTENT_TTL: u32 = 120 * LEDGERS_PER_DAY;
/// Mainnet's minimum temporary entry lifetime: ~24 hours.
pub const MAINNET_MIN_TEMP_TTL: u32 = LEDGERS_PER_DAY;
/// Mainnet's `max_entry_ttl`: ~180 days. No single extension can exceed it.
pub const MAINNET_MAX_ENTRY_TTL: u32 = 180 * LEDGERS_PER_DAY;

/// One month of ledgers — the platform's shortest meaningful access cadence.
pub const MONTH: u32 = 30 * LEDGERS_PER_DAY;
/// A twelve month lease.
pub const ONE_YEAR: u32 = 12 * MONTH;
/// The longest lease the platform underwrites.
pub const TWO_YEARS: u32 = 24 * MONTH;

/// Point `env` at mainnet's state-archival settings, keeping its current
/// sequence number and timestamp.
pub fn set_mainnet_archival_settings(env: &Env) {
    let (sequence_number, timestamp) = (env.ledger().sequence(), env.ledger().timestamp());
    env.ledger().set(LedgerInfo {
        protocol_version: env.ledger().protocol_version(),
        sequence_number,
        timestamp,
        network_id: env.ledger().network_id().to_array(),
        base_reserve: 10,
        min_temp_entry_ttl: MAINNET_MIN_TEMP_TTL,
        min_persistent_entry_ttl: MAINNET_MIN_PERSISTENT_TTL,
        max_entry_ttl: MAINNET_MAX_ENTRY_TTL,
    });
}

/// An `Env` that archives entries the way mainnet does.
pub fn mainnet_env() -> Env {
    let env = Env::default();
    set_mainnet_archival_settings(&env);
    env
}

/// Advance the ledger sequence, moving the clock forward with it so that
/// time-based contract logic stays consistent with the ledger height.
pub fn advance_ledgers(env: &Env, ledgers: u32) {
    let sequence = env.ledger().sequence() + ledgers;
    let timestamp = env.ledger().timestamp() + ledgers as u64 * SECONDS_PER_LEDGER;
    env.ledger().set_sequence_number(sequence);
    env.ledger().set_timestamp(timestamp);
}

/// Keep another contract — and everything it has in persistent storage — alive
/// from a test.
///
/// Useful for contracts a test depends on but does not own: a Stellar Asset
/// Contract token, say, which on a live network is kept alive by its own
/// traffic but in a test ledger only lives as long as the default lifetime.
pub fn keep_contract_alive(env: &Env, contract: &soroban_sdk::Address) {
    use crate::TtlStorage;
    use soroban_sdk::testutils::storage::Persistent as _;

    env.as_contract(contract, || {
        env.extend_instance_ttl();
        // `all()` spans the whole test ledger, so go through the has-guarded
        // helper: keys owned by other contracts are simply skipped.
        for (key, _) in env.storage().persistent().all().iter() {
            env.extend_persistent_ttl(&key);
        }
    });
}
