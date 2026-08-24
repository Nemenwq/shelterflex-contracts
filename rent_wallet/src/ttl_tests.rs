//! Ledger-time TTL tests for `rent_wallet`.
//!
//! Balances live in persistent storage and are read on every rent payment for
//! the length of a lease; admin/pause/cap state lives in instance storage and
//! takes the whole contract with it if it archives. Both are exercised past the
//! network's default 120-day entry lifetime.

use crate::{RentWallet, RentWalletClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};
use soroban_storage_ttl::testutils::{
    advance_ledgers, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

fn setup(env: &Env) -> (RentWalletClient<'_>, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let tenant = Address::generate(env);
    let client = RentWalletClient::new(env, &env.register(RentWallet, ()));
    client.init(&admin);
    (client, admin, tenant)
}

/// A wallet debited once a month keeps its balance entry alive for a 24-month
/// lease, and the wallet still works at the end of it.
#[test]
fn wallet_balance_survives_a_two_year_lease() {
    let env = mainnet_env();
    let (client, admin, tenant) = setup(&env);

    client.credit(&admin, &tenant, &24_000);

    for month in 1..=24i128 {
        advance_ledgers(&env, MONTH);
        client.debit(&admin, &tenant, &1_000);
        assert_eq!(client.balance(&tenant), 24_000 - month * 1_000);
    }

    assert_eq!(client.balance(&tenant), 0);

    // The contract is still fully functional two years in.
    client.credit(&admin, &tenant, &500);
    assert_eq!(client.balance(&tenant), 500);
}

/// A balance that is only read — no writes at all — is still kept alive,
/// because reads extend the entry too.
#[test]
fn reading_a_balance_keeps_it_alive() {
    let env = mainnet_env();
    let (client, admin, tenant) = setup(&env);

    client.credit(&admin, &tenant, &7_500);

    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    assert_eq!(client.balance(&tenant), 7_500);

    // Past the ledger at which an unextended entry would have been archived.
    advance_ledgers(&env, MONTH * 3);
    assert_eq!(client.balance(&tenant), 7_500);
    assert_eq!(client.contract_version(), 1);
}

/// Per-user monthly cap overrides are persistent state read on every debit.
#[test]
fn monthly_cap_override_survives_past_the_default_lifetime() {
    let env = mainnet_env();
    let (client, admin, tenant) = setup(&env);

    client.credit(&admin, &tenant, &50_000);
    client.set_user_monthly_cap(&admin, &tenant, &5_000);

    // Read both the cap and the balance just inside the default lifetime: each
    // read extends the entry it touches.
    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    assert_eq!(client.get_monthly_cap(&tenant), 5_000);
    assert_eq!(client.balance(&tenant), 50_000);

    // Past the ledger at which unextended entries would have been archived.
    advance_ledgers(&env, MONTH * 3);
    assert_eq!(client.get_monthly_cap(&tenant), 5_000);
    client.debit(&admin, &tenant, &5_000);
    assert_eq!(client.get_monthly_spent(&tenant), 5_000);
    assert_eq!(client.balance(&tenant), 45_000);
}
