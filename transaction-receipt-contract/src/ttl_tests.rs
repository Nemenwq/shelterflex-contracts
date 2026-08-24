//! Ledger-time TTL tests for the transaction receipt contract.
//!
//! Receipts are the platform's audit trail: written once, read for years. They
//! are exercised here well past the network's default 120-day entry lifetime.

use crate::{ReceiptInput, TransactionReceiptContract, TransactionReceiptContractClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, String, Symbol};
use soroban_storage_ttl::testutils::{
    advance_ledgers, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

fn setup(env: &Env) -> (TransactionReceiptContractClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let operator = Address::generate(env);
    let client =
        TransactionReceiptContractClient::new(env, &env.register(TransactionReceiptContract, ()));
    client.init(&admin, &operator);
    (client, operator)
}

fn receipt_input(env: &Env, reference: &str) -> ReceiptInput {
    ReceiptInput {
        external_ref_source: Symbol::new(env, "stellar"),
        external_ref: String::from_str(env, reference),
        tx_type: Symbol::new(env, "TENANT_REPAYMENT"),
        amount_usdc: 150_0000000i128,
        token: Address::generate(env),
        deal_id: String::from_str(env, "deal-ttl"),
        listing_id: None,
        from: None,
        to: None,
        amount_ngn: None,
        fx_rate_ngn_per_usdc: None,
        fx_provider: None,
        metadata_hash: None,
    }
}

/// 24 monthly rent receipts: every one of them is still retrievable at the end
/// of the lease, and the deal index still lists them all.
#[test]
fn receipts_survive_a_two_year_lease() {
    let env = mainnet_env();
    let (client, operator) = setup(&env);

    let mut tx_ids: [Option<BytesN<32>>; 24] = Default::default();
    for month in 0..24usize {
        advance_ledgers(&env, MONTH);
        // Distinct references per month so each receipt gets its own tx_id.
        let input = receipt_input(&env, month_reference(month));
        tx_ids[month] = Some(client.record_receipt(&operator, &input));

        // Reading the deal's receipts is what keeps them — and the deal index —
        // alive: every entry the read touches is extended.
        let listed = client.list_receipts_by_deal(&String::from_str(&env, "deal-ttl"), &100, &None);
        assert_eq!(listed.len() as usize, month + 1);
    }

    // Two years on, the very first receipt is still there.
    let first = tx_ids[0].clone().unwrap();
    assert_eq!(client.get_receipt(&first).unwrap().amount_usdc, 150_0000000);
    assert_eq!(
        client
            .list_receipts_by_deal(&String::from_str(&env, "deal-ttl"), &100, &None)
            .len(),
        24
    );
}

/// A single receipt, read once inside the default lifetime, is still readable
/// months after that lifetime would have expired.
#[test]
fn a_receipt_read_once_outlives_the_default_entry_lifetime() {
    let env = mainnet_env();
    let (client, operator) = setup(&env);

    let tx_id = client.record_receipt(&operator, &receipt_input(&env, "rent-ttl-single"));

    // Just inside the default lifetime: reading the receipt and the deal index
    // extends both by another 120 days.
    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    assert!(client.get_receipt(&tx_id).is_some());
    assert_eq!(
        client
            .list_receipts_by_deal(&String::from_str(&env, "deal-ttl"), &10, &None)
            .len(),
        1
    );

    advance_ledgers(&env, MONTH * 3);
    let receipt = client.get_receipt(&tx_id).unwrap();
    assert_eq!(receipt.amount_usdc, 150_0000000);

    // The contract still records new receipts 210 days in.
    let next = client.record_receipt(&operator, &receipt_input(&env, "rent-ttl-single-2"));
    assert!(client.get_receipt(&next).is_some());
}

/// Distinct external references, one per month.
fn month_reference(month: usize) -> &'static str {
    const REFERENCES: [&str; 24] = [
        "rent-ttl-m00",
        "rent-ttl-m01",
        "rent-ttl-m02",
        "rent-ttl-m03",
        "rent-ttl-m04",
        "rent-ttl-m05",
        "rent-ttl-m06",
        "rent-ttl-m07",
        "rent-ttl-m08",
        "rent-ttl-m09",
        "rent-ttl-m10",
        "rent-ttl-m11",
        "rent-ttl-m12",
        "rent-ttl-m13",
        "rent-ttl-m14",
        "rent-ttl-m15",
        "rent-ttl-m16",
        "rent-ttl-m17",
        "rent-ttl-m18",
        "rent-ttl-m19",
        "rent-ttl-m20",
        "rent-ttl-m21",
        "rent-ttl-m22",
        "rent-ttl-m23",
    ];
    REFERENCES[month]
}
