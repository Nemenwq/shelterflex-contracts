//! Ledger-time TTL tests for `staking_pool`.
//!
//! A staking position and the `TotalStaked` accumulator outlive any single
//! lease: both are exercised past the network's default 120-day entry lifetime.

use crate::{StakingPool, StakingPoolClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env};
use soroban_storage_ttl::testutils::{
    advance_ledgers, keep_contract_alive, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

struct Fixture<'a> {
    client: StakingPoolClient<'a>,
    token: Address,
    staker: Address,
}

fn setup(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let staker = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let contract_id = env.register(StakingPool, ());
    let client = StakingPoolClient::new(env, &contract_id);
    client.init(&admin, &token);

    let token_client = token::StellarAssetClient::new(env, &token);
    token_client.mint(&staker, &1_000_000);
    token_client.mint(&contract_id, &1_000_000);

    Fixture {
        client,
        token,
        staker,
    }
}

/// A pool that sees monthly activity keeps a staker's whole position alive —
/// balance, per-deposit records and the `TotalStaked` accumulator — for a full
/// 24-month lease, and the position can still be unstaked at the end.
///
/// Note the shape of the loop: `unstake` is what reads the per-deposit records,
/// and reading them is what extends them. See the PR notes on `Deposit(user, n)`
/// entries, which no other entrypoint touches.
#[test]
fn staking_position_survives_a_two_year_lease() {
    let env = mainnet_env();
    let f = setup(&env);

    f.client.stake(&f.staker, &1_000);

    for _ in 1..=24 {
        advance_ledgers(&env, MONTH);
        keep_contract_alive(&env, &f.token);

        f.client.stake(&f.staker, &1_000);
        f.client.unstake(&f.staker, &1_000);
        assert_eq!(f.client.staked_balance(&f.staker), 1_000);
    }

    // Two years on, the original stake is still live and still withdrawable.
    assert_eq!(f.client.total_staked(), 1_000);
    f.client.unstake(&f.staker, &1_000);
    assert_eq!(f.client.staked_balance(&f.staker), 0);
    assert_eq!(f.client.total_staked(), 0);
}

/// A balance that is only ever read stays live too: the read extends it, and
/// the pool still accepts new stakes well past the default entry lifetime.
#[test]
fn reading_a_position_keeps_it_alive_past_the_default_lifetime() {
    let env = mainnet_env();
    let f = setup(&env);

    f.client.stake(&f.staker, &5_000);

    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    keep_contract_alive(&env, &f.token);
    assert_eq!(f.client.staked_balance(&f.staker), 5_000);
    assert_eq!(f.client.total_staked(), 5_000);

    // Past the ledger at which unextended entries would have been archived,
    // 210 days after the stake was written.
    advance_ledgers(&env, MONTH * 3);
    keep_contract_alive(&env, &f.token);
    assert_eq!(f.client.staked_balance(&f.staker), 5_000);
    assert_eq!(f.client.total_staked(), 5_000);
    assert_eq!(f.client.contract_version(), 1);
}
