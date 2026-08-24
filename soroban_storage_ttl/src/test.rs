//! Ledger-time tests for the TTL policy.
//!
//! Every test here advances the ledger sequence far past the network's default
//! entry lifetime; a test that never advances the ledger cannot catch an
//! archival bug.

use crate::testutils::{
    advance_ledgers, mainnet_env, LEDGERS_PER_DAY, MAINNET_MAX_ENTRY_TTL,
    MAINNET_MIN_PERSISTENT_TTL, MONTH,
};
use crate::{TtlStorage, PERSISTENT_BUMP_TO, TEMPORARY_BUMP_TO};
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::storage::{Instance as _, Persistent as _, Temporary as _},
    Env,
};

#[contracttype]
#[derive(Clone)]
pub enum Key {
    Deal,
    Nonce,
}

#[contract]
pub struct TtlTestContract;

#[contractimpl]
impl TtlTestContract {
    /// Writes through the policy helpers, the way a contract entrypoint should.
    pub fn write(env: Env, value: i128) {
        env.extend_instance_ttl();
        env.set_persistent(&Key::Deal, &value);
        env.storage().instance().set(&Key::Nonce, &value);
    }

    /// Reads through the policy helpers, which keeps the entry alive.
    pub fn read(env: Env) -> Option<i128> {
        env.extend_instance_ttl();
        env.get_persistent(&Key::Deal)
    }

    /// Writes *without* the policy helpers — the pre-fix behaviour, kept as a
    /// negative control.
    pub fn write_unmanaged(env: Env, value: i128) {
        env.storage().persistent().set(&Key::Deal, &value);
    }

    /// Reads *without* the policy helpers.
    pub fn read_unmanaged(env: Env) -> Option<i128> {
        env.storage().persistent().get(&Key::Deal)
    }

    pub fn write_temporary(env: Env, value: i128) {
        env.set_temporary(&Key::Nonce, &value);
    }

    pub fn read_temporary(env: Env) -> Option<i128> {
        env.get_temporary(&Key::Nonce)
    }
}

#[test]
fn policy_constants_fit_under_the_mainnet_cap() {
    // Checked at compile time: an extend-to target above `max_entry_ttl` is
    // clamped for persistent/instance entries and an outright error for
    // temporary ones, and a threshold above its target is rejected by the host.
    const {
        assert!(PERSISTENT_BUMP_TO <= MAINNET_MAX_ENTRY_TTL);
        assert!(TEMPORARY_BUMP_TO <= MAINNET_MAX_ENTRY_TTL);
        assert!(crate::INSTANCE_BUMP_TO <= MAINNET_MAX_ENTRY_TTL);
        assert!(crate::PERSISTENT_BUMP_THRESHOLD < PERSISTENT_BUMP_TO);
        assert!(crate::INSTANCE_BUMP_THRESHOLD < crate::INSTANCE_BUMP_TO);
        assert!(crate::TEMPORARY_BUMP_THRESHOLD < TEMPORARY_BUMP_TO);
        // A monthly access cadence must leave a wide margin.
        assert!(PERSISTENT_BUMP_TO >= 3 * MONTH);
    }
}

#[test]
fn monthly_access_keeps_an_entry_alive_for_a_two_year_lease() {
    let env = mainnet_env();
    let id = env.register(TtlTestContract, ());
    let client = TtlTestContractClient::new(&env, &id);

    client.write(&42);

    // 24 monthly payments: read the deal once a month for two years.
    for _ in 0..24 {
        advance_ledgers(&env, MONTH);
        assert_eq!(client.read(), Some(42));
    }

    // Two years on, the entry is still live and the contract still works.
    assert_eq!(client.read(), Some(42));
    client.write(&43);
    assert_eq!(client.read(), Some(43));
}

#[test]
fn reading_extends_the_entry_beyond_the_default_lifetime() {
    let env = mainnet_env();
    let id = env.register(TtlTestContract, ());
    let client = TtlTestContractClient::new(&env, &id);

    client.write(&7);

    // Sit just inside the default lifetime, then read.
    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    assert_eq!(client.read(), Some(7));

    // The read pushed the entry back out to the policy target.
    env.as_contract(&id, || {
        assert_eq!(
            env.storage().persistent().get_ttl(&Key::Deal),
            PERSISTENT_BUMP_TO
        );
        assert_eq!(env.storage().instance().get_ttl(), crate::INSTANCE_BUMP_TO);
    });

    // Past the point where an unmanaged entry would have been archived.
    advance_ledgers(&env, MONTH * 3);
    assert_eq!(client.read(), Some(7));
}

#[test]
#[should_panic]
fn unmanaged_entry_is_archived_after_the_default_lifetime() {
    let env = mainnet_env();
    let id = env.register(TtlTestContract, ());
    let client = TtlTestContractClient::new(&env, &id);

    client.write_unmanaged(&7);
    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL + 1);

    // Negative control: without TTL management this read fails on the archived
    // entry. If this test ever stops panicking, the ledger is not really being
    // advanced and the other tests here prove nothing.
    client.read_unmanaged();
}

#[test]
fn temporary_entries_are_extended_within_their_shorter_window() {
    let env = mainnet_env();
    let id = env.register(TtlTestContract, ());
    let client = TtlTestContractClient::new(&env, &id);

    client.write_temporary(&5);
    env.as_contract(&id, || {
        assert_eq!(
            env.storage().temporary().get_ttl(&Key::Nonce),
            TEMPORARY_BUMP_TO
        );
    });

    // Well past the 24h default temporary lifetime.
    advance_ledgers(&env, 20 * LEDGERS_PER_DAY);
    assert_eq!(client.read_temporary(), Some(5));
}
