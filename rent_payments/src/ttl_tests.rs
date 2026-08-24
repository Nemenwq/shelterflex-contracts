//! Ledger-time TTL tests for `rent_payments`.
//!
//! A deal's receipt ledger is appended to on every monthly rent payment and
//! must still be readable at the end of a 12–24 month lease.

use crate::{RentPayments, RentPaymentsClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};
use soroban_storage_ttl::testutils::{
    advance_ledgers, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

const DEAL: u64 = 77;

fn reference(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    BytesN::from_array(env, &bytes)
}

fn setup(env: &Env) -> (RentPaymentsClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let client = RentPaymentsClient::new(env, &env.register(RentPayments, ()));
    client.init(&admin);
    (client, admin)
}

/// 24 monthly receipts: the receipt list and the used-reference set are still
/// live at the end of the lease, and the contract still accepts payments.
#[test]
fn receipt_ledger_survives_a_two_year_lease() {
    let env = mainnet_env();
    let (client, payer) = setup(&env);

    for month in 1..=24u8 {
        advance_ledgers(&env, MONTH);
        let receipt = client.create_receipt(&DEAL, &1_000, &payer, &reference(&env, month));
        assert_eq!(receipt.id, u64::from(month));
    }

    assert_eq!(client.receipt_count(&DEAL), 24);
    let page = client.list_receipts_by_deal(&DEAL, &100, &None);
    assert_eq!(page.receipts.len(), 24);
}

/// A deal touched at a monthly-or-slower cadence outlives the network's
/// default 120-day entry lifetime: each payment extends the receipt ledger,
/// the receipt counter and the transaction-id counter.
#[test]
fn receipts_outlive_the_default_entry_lifetime() {
    let env = mainnet_env();
    let (client, payer) = setup(&env);

    client.create_receipt(&DEAL, &2_500, &payer, &reference(&env, 1));

    // Just inside the default lifetime: this payment extends every entry it
    // touches by another 120 days.
    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    client.create_receipt(&DEAL, &2_500, &payer, &reference(&env, 2));

    // Three months later — 209 days in, well past the default lifetime — the
    // ledger is still readable and the contract still accepts payments.
    advance_ledgers(&env, MONTH * 3);
    let page = client.list_receipts_by_deal(&DEAL, &10, &None);
    assert_eq!(page.receipts.len(), 2);
    assert_eq!(page.receipts.get(0).unwrap().amount, 2_500);

    client.create_receipt(&DEAL, &1_000, &payer, &reference(&env, 3));
    assert_eq!(client.receipt_count(&DEAL), 3);

    // A replay of a recently written reference is still rejected.
    assert!(client
        .try_create_receipt(&DEAL, &1_000, &payer, &reference(&env, 3))
        .is_err());
}
