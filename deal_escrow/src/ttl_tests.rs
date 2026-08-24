//! Ledger-time TTL tests for `deal_escrow`.
//!
//! Every test runs against mainnet's state-archival settings and advances the
//! ledger past the default 120-day entry lifetime. A test that never advances
//! the ledger cannot catch an archival bug.

use crate::{DealEscrow, DealEscrowClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env, String, Symbol};
use soroban_storage_ttl::testutils::{
    advance_ledgers, keep_contract_alive, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

struct Fixture<'a> {
    client: DealEscrowClient<'a>,
    token: Address,
    operator: Address,
    depositor: Address,
    landlord: Address,
    platform: Address,
    reporter: Address,
}

fn setup(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let operator = Address::generate(env);
    let depositor = Address::generate(env);
    let landlord = Address::generate(env);
    let platform = Address::generate(env);
    let reporter = Address::generate(env);
    let receipt_contract = Address::generate(env);

    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(env, &token).mint(&depositor, &10_000_000);

    let client = DealEscrowClient::new(env, &env.register(DealEscrow, ()));
    client.init(&admin, &operator, &token, &receipt_contract);

    Fixture {
        client,
        token,
        operator,
        depositor,
        landlord,
        platform,
        reporter,
    }
}

/// A deal paid into once a month survives a full 24-month lease — balance,
/// depositor and deal state are all still live — and can still be settled.
#[test]
fn escrowed_deal_survives_a_two_year_lease() {
    let env = mainnet_env();
    let f = setup(&env);
    let deal_id = String::from_str(&env, "deal-ttl-1");

    for month in 1..=24u32 {
        advance_ledgers(&env, MONTH);
        // The token is an external contract; on a live network its own traffic
        // keeps it alive.
        keep_contract_alive(&env, &f.token);

        f.client.deposit(&f.depositor, &deal_id, &1_000);
        assert_eq!(f.client.balance(&deal_id), i128::from(month) * 1_000);
    }

    // Two years on, the deal entries are still live and settlement works.
    let released = f.client.release(
        &f.operator,
        &deal_id,
        &f.landlord,
        &20_000,
        &f.platform,
        &3_000,
        &f.reporter,
        &1_000,
        &Symbol::new(&env, "rent"),
        &String::from_str(&env, "ref-ttl-1"),
    );
    assert_eq!(released, 24_000);
    assert_eq!(f.client.balance(&deal_id), 0);
}

/// Instance storage (admin, operator, token, paused, schema version) shares the
/// contract instance's TTL: if it lapses, *every* entrypoint fails. Each call
/// bumps it, so the contract is still callable two years in.
#[test]
fn contract_instance_survives_a_two_year_lease() {
    let env = mainnet_env();
    let f = setup(&env);
    let deal_id = String::from_str(&env, "deal-ttl-2");

    for _ in 0..24 {
        advance_ledgers(&env, MONTH);
        keep_contract_alive(&env, &f.token);
        assert_eq!(f.client.contract_version(), 1);
    }

    assert_eq!(f.client.storage_schema_version(), crate::STORAGE_SCHEMA_V3);

    f.client.deposit(&f.depositor, &deal_id, &5_000);
    assert_eq!(f.client.balance(&deal_id), 5_000);
}

/// A balance written once and left alone is still readable well past the
/// network's default entry lifetime: the write extended it, and each read
/// extends it again.
#[test]
fn deposited_balance_outlives_the_default_entry_lifetime() {
    let env = mainnet_env();
    let f = setup(&env);
    let deal_id = String::from_str(&env, "deal-ttl-3");

    f.client.deposit(&f.depositor, &deal_id, &42_000);

    // Sit just inside the 120-day target, then read: that read pushes it out
    // by another 120 days.
    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    assert_eq!(f.client.balance(&deal_id), 42_000);

    // Past the ledger at which an unextended entry would have been archived.
    advance_ledgers(&env, MONTH * 3);
    assert_eq!(f.client.balance(&deal_id), 42_000);
}
