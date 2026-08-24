//! Ledger-time TTL tests for `bond_collateral`.
//!
//! A collateral position is written when a bond is issued and may sit for the
//! length of the bond before it is redeemed; the thresholds and the
//! `TotalCollateral` accumulator live in instance storage.

use crate::{BondCollateral, BondCollateralClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, BytesN, Env};
use soroban_storage_ttl::testutils::{
    advance_ledgers, keep_contract_alive, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

fn position_id(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    BytesN::from_array(env, &bytes)
}

struct Fixture<'a> {
    client: BondCollateralClient<'a>,
    token: Address,
    owner: Address,
}

fn setup(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let owner = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let contract_id = env.register(BondCollateral, ());
    let client = BondCollateralClient::new(env, &contract_id);
    client.init(&admin, &token);

    let token_client = StellarAssetClient::new(env, &token);
    token_client.mint(&owner, &1_000_000);
    token_client.mint(&contract_id, &1_000_000);

    Fixture {
        client,
        token,
        owner,
    }
}

/// A bond posted at lease start is still live — and still redeemable — after a
/// 24-month term of monthly top-ups.
#[test]
fn collateral_position_survives_a_two_year_bond() {
    let env = mainnet_env();
    let f = setup(&env);
    let id = position_id(&env, 1);

    f.client.deposit_collateral(&f.owner, &id, &100_000);
    f.client.issue_bond(&f.owner, &id, &10_000);

    for month in 1..=24i128 {
        advance_ledgers(&env, MONTH);
        keep_contract_alive(&env, &f.token);

        f.client.deposit_collateral(&f.owner, &id, &1_000);
        let position = f.client.get_position(&id).unwrap();
        assert_eq!(position.collateral_amount, 100_000 + month * 1_000);
    }

    assert_eq!(f.client.total_collateral(), 124_000);
    f.client.redeem_bond(&f.owner, &id, &10_000);
    assert_eq!(f.client.get_position(&id).unwrap().bond_amount, 0);
}

/// A position that is only read is kept alive by the read alone, well past the
/// network's default entry lifetime.
#[test]
fn reading_a_position_keeps_it_alive_past_the_default_lifetime() {
    let env = mainnet_env();
    let f = setup(&env);
    let id = position_id(&env, 2);

    f.client.deposit_collateral(&f.owner, &id, &50_000);
    f.client.issue_bond(&f.owner, &id, &5_000);

    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    keep_contract_alive(&env, &f.token);
    assert!(f.client.get_position(&id).is_some());
    assert!(f.client.get_collateral_ratio(&id).is_some());

    // Past the ledger at which an unextended position would have been archived.
    advance_ledgers(&env, MONTH * 3);
    keep_contract_alive(&env, &f.token);
    assert_eq!(
        f.client.get_position(&id).unwrap().collateral_amount,
        50_000
    );
    assert_eq!(f.client.get_thresholds(), (150, 120));
    f.client.redeem_bond(&f.owner, &id, &5_000);
}
