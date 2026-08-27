//! Transaction Receipt Contract
//!
//! This contract provides deterministic transaction receipt recording and
//! retrieval for on-chain indexing. Receipts are keyed by a SHA-256 hash of a
//! canonicalized external payment reference (the `tx_id`). The contract enforces
//! validation rules on external references, prevents duplicates, and supports
//! admin/operator authorization and pause control.
//!
#![no_std]

use soroban_pausable_core::{Pausable, PausableError};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, String, Symbol,
};
use soroban_storage_ttl::TtlStorage;

#[cfg(kani)]
pub mod formal_properties;

/// Allowed external reference sources for transaction ID generation
pub const ALLOWED_SOURCES: [&str; 8] = [
    "paystack",
    "flutterwave",
    "bank_transfer",
    "stellar",
    "onramp",
    "offramp",
    "manual",
    "manual_admin",
];

/// Allowed transaction types for MVP
pub const ALLOWED_TX_TYPES: [&str; 7] = [
    "TENANT_REPAYMENT",
    "LANDLORD_PAYOUT",
    "WHISTLEBLOWER_REWARD",
    "STAKE",
    "UNSTAKE",
    "STAKE_REWARD_CLAIM",
    "CONVERSION",
];

/// Input parameters for recording a receipt (to avoid 10-parameter limit)
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptInput {
    /// The payment source (e.g., "paystack", "stellar")
    pub external_ref_source: Symbol,
    /// The external payment reference string
    pub external_ref: String,
    /// Transaction type (e.g., "rent_payment", "deposit", "refund")
    pub tx_type: Symbol,
    /// Transaction amount in USDC (canonical amount, must be positive)
    pub amount_usdc: i128,
    /// USDC token contract address
    pub token: Address,
    /// Deal identifier this transaction belongs to
    pub deal_id: String,
    /// Optional listing identifier
    pub listing_id: Option<String>,
    /// Optional sender address
    pub from: Option<Address>,
    /// Optional recipient address
    pub to: Option<Address>,
    /// Optional amount in NGN (metadata only)
    pub amount_ngn: Option<i128>,
    /// Optional FX rate (NGN per USDC, metadata only)
    pub fx_rate_ngn_per_usdc: Option<i128>,
    /// Optional FX provider name (metadata only)
    pub fx_provider: Option<String>,
    /// Optional metadata hash (SHA-256 of canonical receipt payload v1)
    pub metadata_hash: Option<BytesN<32>>,
}

/// Extract the raw content bytes of a Soroban `String` as a host-backed
/// `Bytes` object, without copying through guest (wasm linear) memory.
///
/// A `String`'s XDR encoding is `[4-byte SCV_STRING discriminant][4-byte
/// length][content bytes][zero padding to a 4-byte boundary]`. Slicing the
/// XDR bytes from offset 8 for `len()` bytes yields exactly the content,
/// regardless of string length — this avoids needing a fixed-size local
/// buffer to support arbitrary-length fields like `deal_id`.
fn string_raw_bytes(env: &soroban_sdk::Env, s: &String) -> soroban_sdk::Bytes {
    use soroban_sdk::xdr::ToXdr;

    let xdr = s.to_val().to_xdr(env);
    let len = s.len();
    xdr.slice(8..8 + len)
}

fn is_ascii_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// Trim leading/trailing ASCII whitespace from a Soroban `String`, returning
/// the trimmed content as a host-backed `Bytes` object.
fn trim_ascii_bytes(env: &soroban_sdk::Env, s: &String) -> soroban_sdk::Bytes {
    let content = string_raw_bytes(env, s);
    let len = content.len();

    let mut start: u32 = 0;
    while start < len && is_ascii_ws(content.get_unchecked(start)) {
        start += 1;
    }

    let mut end: u32 = len;
    while end > start && is_ascii_ws(content.get_unchecked(end - 1)) {
        end -= 1;
    }

    content.slice(start..end)
}

/// Extract a Symbol's exact-case ASCII characters into a stack buffer.
/// Symbols are capped at 32 characters by the SDK, so a fixed buffer is safe
/// (unlike arbitrary-length `String` fields).
fn symbol_ascii(env: &soroban_sdk::Env, sym: &Symbol) -> ([u8; 32], usize) {
    use soroban_sdk::{SymbolStr, TryFromVal};

    let ss = SymbolStr::try_from_val(env, &sym.to_symbol_val()).unwrap_or_default();
    let raw: &[u8] = ss.as_ref();
    let len = raw.len();
    let mut buf = [0u8; 32];
    buf[..len].copy_from_slice(raw);
    (buf, len)
}

/// Validate `external_ref_source` against `ALLOWED_SOURCES` case-insensitively
/// and return the matching canonical (lowercase) source string.
fn normalize_source(
    env: &soroban_sdk::Env,
    external_ref_source: &Symbol,
) -> Result<&'static str, ContractError> {
    let (buf, len) = symbol_ascii(env, external_ref_source);
    let mut lower = [0u8; 32];
    for i in 0..len {
        lower[i] = buf[i].to_ascii_lowercase();
    }
    let lowered = &lower[..len];

    for allowed in ALLOWED_SOURCES.iter() {
        if lowered == allowed.as_bytes() {
            return Ok(allowed);
        }
    }

    Err(ContractError::InvalidExternalRefSource)
}

/// Validate and normalize an `(external_ref_source, external_ref)` pair per
/// the v1 canonicalization rules, returning the lowercased source and the
/// trimmed reference content.
///
/// # Validation Rules
/// * `external_ref_source` must match `ALLOWED_SOURCES` case-insensitively
/// * `external_ref` must not be empty after trimming ASCII whitespace
/// * `external_ref` must not contain a pipe character (`|`) after trimming
/// * `external_ref` must not exceed 256 characters after trimming (character
///   count, not XDR byte length)
fn validate_and_normalize_ref(
    env: &soroban_sdk::Env,
    external_ref_source: &Symbol,
    external_ref: &String,
) -> Result<(&'static str, soroban_sdk::Bytes), ContractError> {
    let source_lower = normalize_source(env, external_ref_source)?;
    let ref_trimmed = trim_ascii_bytes(env, external_ref);

    if ref_trimmed.is_empty() {
        return Err(ContractError::InvalidExternalRef);
    }

    if ref_trimmed.len() > 256 {
        return Err(ContractError::InvalidExternalRef);
    }

    for b in ref_trimmed.iter() {
        if b == b'|' {
            return Err(ContractError::InvalidExternalRef);
        }
    }

    Ok((source_lower, ref_trimmed))
}

/// Helper function to validate external reference source and external reference.
///
/// This enforces the same invariants as `generate_tx_id`, but can be used
/// independently in validation flows.
fn validate_external_ref(
    env: &soroban_sdk::Env,
    external_ref_source: &Symbol,
    external_ref: &String,
) -> Result<(), ContractError> {
    validate_and_normalize_ref(env, external_ref_source, external_ref).map(|_| ())
}

/// Append the decimal ASCII representation of an `i128` to `combined`
/// (e.g. `-42` or `0`), using `u128` magnitude arithmetic so `i128::MIN`
/// does not overflow on negation.
fn append_i128_decimal(combined: &mut soroban_sdk::Bytes, value: i128) {
    if value == 0 {
        combined.push_back(b'0');
        return;
    }

    let negative = value < 0;
    if negative {
        combined.push_back(b'-');
    }

    let mut magnitude: u128 = value.unsigned_abs();
    let mut digits: [u8; 40] = [0; 40];
    let mut pos = 0;
    while magnitude > 0 {
        digits[pos] = (magnitude % 10) as u8 + b'0';
        magnitude /= 10;
        pos += 1;
    }
    for i in (0..pos).rev() {
        combined.push_back(digits[i]);
    }
}

/// Produce canonical bytes for metadata hashing (v1).
///
/// Canonical format:
/// `v1|external_ref_source=<lowercased_trimmed>|external_ref=<trimmed>|tx_type=<case_sensitive>|amount_usdc=<i128>|token=<address>|deal_id=<string>|listing_id=<string>|from=<address>|to=<address>|amount_ngn=<i128>|fx_rate_ngn_per_usdc=<i128>|fx_provider=<string>`
///
/// Optional fields rules:
/// - If `None`, the key is omitted entirely.
/// - If `Some`, values are rendered without extra whitespace.
///
/// Ordering is fixed and MUST NOT change.
fn canonical_metadata_payload_v1(
    env: &soroban_sdk::Env,
    input: &ReceiptInput,
) -> soroban_sdk::Bytes {
    use soroban_sdk::Bytes;

    let mut combined = Bytes::new(env);

    combined.extend_from_slice(b"v1|external_ref_source=");
    let source_lower = normalize_source(env, &input.external_ref_source).unwrap_or_default();
    combined.extend_from_slice(source_lower.as_bytes());

    combined.extend_from_slice(b"|external_ref=");
    combined.append(&trim_ascii_bytes(env, &input.external_ref));

    combined.extend_from_slice(b"|tx_type=");
    let (tx_type_buf, tx_type_len) = symbol_ascii(env, &input.tx_type);
    combined.extend_from_slice(&tx_type_buf[..tx_type_len]);

    combined.extend_from_slice(b"|amount_usdc=");
    append_i128_decimal(&mut combined, input.amount_usdc);

    combined.extend_from_slice(b"|token=");
    combined.append(&string_raw_bytes(env, &input.token.to_string()));

    combined.extend_from_slice(b"|deal_id=");
    combined.append(&string_raw_bytes(env, &input.deal_id));

    if let Some(ref listing_id) = input.listing_id {
        combined.extend_from_slice(b"|listing_id=");
        combined.append(&string_raw_bytes(env, listing_id));
    }

    if let Some(ref from) = input.from {
        combined.extend_from_slice(b"|from=");
        combined.append(&string_raw_bytes(env, &from.to_string()));
    }

    if let Some(ref to) = input.to {
        combined.extend_from_slice(b"|to=");
        combined.append(&string_raw_bytes(env, &to.to_string()));
    }

    if let Some(amount_ngn) = input.amount_ngn {
        combined.extend_from_slice(b"|amount_ngn=");
        append_i128_decimal(&mut combined, amount_ngn);
    }

    if let Some(fx_rate) = input.fx_rate_ngn_per_usdc {
        combined.extend_from_slice(b"|fx_rate_ngn_per_usdc=");
        append_i128_decimal(&mut combined, fx_rate);
    }

    if let Some(ref fx_provider) = input.fx_provider {
        combined.extend_from_slice(b"|fx_provider=");
        combined.append(&string_raw_bytes(env, fx_provider));
    }

    combined
}

fn derive_metadata_hash(env: &soroban_sdk::Env, input: &ReceiptInput) -> BytesN<32> {
    let payload = canonical_metadata_payload_v1(env, input);
    env.crypto().sha256(&payload).into()
}

fn verify_metadata_hash(env: &soroban_sdk::Env, input: &ReceiptInput, hash: &BytesN<32>) -> bool {
    derive_metadata_hash(env, input) == hash.clone()
}

/// Receipt data structure representing an immutable transaction record
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// Unique transaction identifier (SHA-256 hash of canonical external reference)
    pub tx_id: BytesN<32>,
    /// Transaction type (e.g., "rent_payment", "deposit", "refund")
    pub tx_type: Symbol,
    /// Transaction amount in USDC (canonical amount, must be positive)
    pub amount_usdc: i128,
    /// USDC token contract address
    pub token: Address,
    /// Deal identifier this transaction belongs to
    pub deal_id: String,
    /// Optional listing identifier
    pub listing_id: Option<String>,
    /// Optional sender address
    pub from: Option<Address>,
    /// Optional recipient address
    pub to: Option<Address>,
    /// External reference (same as tx_id)
    pub external_ref: BytesN<32>,
    /// Optional amount in NGN (metadata only)
    pub amount_ngn: Option<i128>,
    /// Optional FX rate (NGN per USDC, metadata only)
    pub fx_rate_ngn_per_usdc: Option<i128>,
    /// Optional FX provider name (metadata only)
    pub fx_provider: Option<String>,
    /// Optional metadata hash (SHA-256 of canonical receipt payload v1)
    pub metadata_hash: Option<BytesN<32>>,
    /// Timestamp when receipt was recorded (ledger timestamp)
    pub timestamp: u64,
}

/// Storage keys for contract state
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageKey {
    ContractVersion,
    /// Admin address (set during initialization, immutable)
    Admin,
    /// Operator address (can be changed by admin)
    Operator,
    /// Paused state (boolean)
    Paused,
    /// Receipt storage: tx_id → Receipt
    Receipt(BytesN<32>),
    /// Deal index: (deal_id, index) → tx_id
    DealIndex(String, u32),
    /// Deal count: deal_id → count
    DealCount(String),
    /// User index: (user_address, index) → tx_id
    UserIndex(Address, u32),
    /// User count: user_address → count
    UserCount(Address),
}

/// Contract error types
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    /// Contract has already been initialized
    AlreadyInitialized = 1,
    /// Caller is not authorized for this operation
    NotAuthorized = 2,
    /// Contract is currently paused
    Paused = 3,
    /// Amount is invalid (zero or negative)
    InvalidAmount = 4,
    /// Transaction ID already exists (duplicate)
    DuplicateTransaction = 5,
    /// External reference source is not in allowed list
    InvalidExternalRefSource = 6,
    /// External reference is invalid (empty, contains pipes, or too long)
    InvalidExternalRef = 7,
    /// Timestamp is invalid
    InvalidTimestamp = 8,
    /// Transaction type is not in allowed list
    InvalidTxType = 9,
    /// Metadata hash is invalid (does not match canonical payload)
    InvalidMetadataHash = 10,
}

#[contract]
/// Primary contract type. All public contract methods are implemented on this
/// struct via the `#[contractimpl]` impl block.
pub struct TransactionReceiptContract;

#[contractimpl]
impl TransactionReceiptContract {
    /// Initialize the contract with admin and operator addresses
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `admin` - The admin address (can manage operator and pause state)
    /// * `operator` - The operator address (can record receipts)
    ///
    /// # Returns
    /// * `Ok(())` - If initialization succeeds
    /// * `Err(ContractError::AlreadyInitialized)` - If contract is already initialized
    ///
    /// # Requirements
    /// * Can only be called once (Requirements 1.3)
    /// * Stores admin and operator addresses (Requirements 1.1, 1.2)
    /// * Initializes paused state to false
    pub fn init(
        env: soroban_sdk::Env,
        admin: Address,
        operator: Address,
    ) -> Result<(), ContractError> {
        env.extend_instance_ttl();

        // Check if already initialized by checking if Admin key exists
        if env.storage().instance().has(&StorageKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }

        // Store admin address
        env.storage().instance().set(&StorageKey::Admin, &admin);

        // Store operator address
        env.storage()
            .instance()
            .set(&StorageKey::Operator, &operator);

        env.storage()
            .instance()
            .set(&StorageKey::ContractVersion, &1u32);

        // Initialize paused state to false
        env.storage().instance().set(&StorageKey::Paused, &false);

        env.events().publish(
            (
                Symbol::new(&env, "transaction_receipt"),
                Symbol::new(&env, "init"),
            ),
            (admin, operator, 1u32),
        );

        Ok(())
    }

    pub fn contract_version(env: soroban_sdk::Env) -> u32 {
        env.extend_instance_ttl();

        env.storage()
            .instance()
            .get::<_, u32>(&StorageKey::ContractVersion)
            .unwrap_or(0u32)
    }

    pub fn version(env: soroban_sdk::Env) -> u32 {
        env.extend_instance_ttl();

        Self::contract_version(env)
    }

    /// Set a new operator address
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `admin` - The admin address attempting to set operator
    /// * `new_operator` - The new operator address
    ///
    /// # Returns
    /// * `Ok(())` - If operator update succeeds
    /// * `Err(ContractError::NotAuthorized)` - If caller is not admin
    ///
    /// # Requirements
    /// * Only admin can set operator (Requirement 5.2, 7.2)
    /// * Updates operator address in storage (Requirement 7.1)
    /// * Accepts any valid Soroban Address (Requirement 7.3)
    pub fn set_operator(
        env: soroban_sdk::Env,
        admin: Address,
        new_operator: Address,
    ) -> Result<(), ContractError> {
        env.extend_instance_ttl();

        // Require authentication from the admin
        admin.require_auth();

        // Verify caller is admin
        require_admin(&env, &admin)?;

        // Update operator address in storage
        let old_operator: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Operator)
            .unwrap_or_else(|| panic!("Operator not initialized"));
        env.storage()
            .instance()
            .set(&StorageKey::Operator, &new_operator);

        env.events().publish(
            (
                Symbol::new(&env, "transaction_receipt"),
                Symbol::new(&env, "set_operator"),
            ),
            (old_operator, new_operator),
        );

        Ok(())
    }

    /// Record a new transaction receipt
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `operator` - The operator address attempting to record
    /// * `input` - Receipt input parameters (ReceiptInput struct)
    ///
    /// # Returns
    /// * `Ok(BytesN<32>)` - The generated tx_id
    /// * `Err(ContractError)` - If validation fails or duplicate detected
    ///
    /// # Requirements
    /// * Only operator can record (Requirement 5.1)
    /// * Contract must not be paused (Requirement 6.2)
    /// * Amount must be positive (Requirement 2.4)
    /// * Rejects duplicate tx_id (Requirement 3.1)
    /// * Emits event on success (Requirements 10.1, 10.2, 10.3)
    pub fn record_receipt(
        env: soroban_sdk::Env,
        operator: Address,
        input: ReceiptInput,
    ) -> Result<BytesN<32>, ContractError> {
        env.extend_instance_ttl();

        // Require authentication from the operator
        operator.require_auth();

        // Verify caller is operator
        require_operator(&env, &operator)?;

        // Verify contract is not paused
        require_not_paused(&env)?;

        // Validate amount_usdc is positive
        if input.amount_usdc <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        // Validate tx_type is in allowed list
        validate_tx_type(&env, &input.tx_type)?;

        // Validate external reference source and reference
        validate_external_ref(&env, &input.external_ref_source, &input.external_ref)?;

        // Generate tx_id from canonical external reference
        let tx_id = generate_tx_id(&env, &input.external_ref_source, &input.external_ref)?;

        // If provided, validate metadata hash against canonical payload
        if let Some(ref mh) = input.metadata_hash {
            if !verify_metadata_hash(&env, &input, mh) {
                return Err(ContractError::InvalidMetadataHash);
            }
        }

        // Check for duplicate tx_id
        if env.has_persistent(&StorageKey::Receipt(tx_id.clone())) {
            return Err(ContractError::DuplicateTransaction);
        }

        // Get current ledger timestamp
        let timestamp = env.ledger().timestamp();

        // Create Receipt struct
        let receipt = Receipt {
            tx_id: tx_id.clone(),
            tx_type: input.tx_type,
            amount_usdc: input.amount_usdc,
            token: input.token,
            deal_id: input.deal_id.clone(),
            listing_id: input.listing_id,
            from: input.from,
            to: input.to,
            external_ref: tx_id.clone(), // Same as tx_id per Requirement 4.10
            amount_ngn: input.amount_ngn,
            fx_rate_ngn_per_usdc: input.fx_rate_ngn_per_usdc,
            fx_provider: input.fx_provider,
            metadata_hash: input.metadata_hash,
            timestamp,
        };

        // Store receipt in persistent storage
        env.set_persistent(&StorageKey::Receipt(tx_id.clone()), &receipt);

        // Update deal index
        let deal_count_key = StorageKey::DealCount(input.deal_id.clone());
        let current_count: u32 = env.get_persistent(&deal_count_key).unwrap_or(0);

        // Store tx_id in deal index
        let deal_index_key = StorageKey::DealIndex(input.deal_id.clone(), current_count);
        env.set_persistent(&deal_index_key, &tx_id);

        // Increment deal count
        env.set_persistent(&deal_count_key, &(current_count + 1));

        // Update user indices for from and to addresses
        if let Some(ref from_addr) = receipt.from {
            let user_count_key = StorageKey::UserCount(from_addr.clone());
            let user_count: u32 = env.get_persistent(&user_count_key).unwrap_or(0);
            env.set_persistent(
                &StorageKey::UserIndex(from_addr.clone(), user_count),
                &tx_id,
            );
            env.set_persistent(&user_count_key, &(user_count + 1));
        }

        if let Some(ref to_addr) = receipt.to {
            let user_count_key = StorageKey::UserCount(to_addr.clone());
            let user_count: u32 = env.get_persistent(&user_count_key).unwrap_or(0);
            env.set_persistent(&StorageKey::UserIndex(to_addr.clone(), user_count), &tx_id);
            env.set_persistent(&user_count_key, &(user_count + 1));
        }

        // Emit event with topic ("receipt", tx_id) and receipt payload
        env.events().publish(
            (
                Symbol::new(&env, "transaction_receipt"),
                Symbol::new(&env, "receipt_recorded"),
                tx_id.clone(),
            ),
            receipt,
        );

        Ok(tx_id)
    }

    /// Retrieve a receipt by transaction ID
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `tx_id` - The transaction ID to look up
    ///
    /// # Returns
    /// * `Some(Receipt)` - If the receipt exists
    /// * `None` - If the receipt does not exist
    ///
    /// # Requirements
    /// * Returns complete receipt if exists (Requirement 8.1, 8.3)
    /// * Returns None for non-existent tx_id (Requirement 8.2)
    /// * No authorization required (public read)
    pub fn get_receipt(env: soroban_sdk::Env, tx_id: BytesN<32>) -> Option<Receipt> {
        env.extend_instance_ttl();

        env.get_persistent(&StorageKey::Receipt(tx_id))
    }

    /// List all receipts for a specific deal with pagination
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `deal_id` - The deal identifier to query
    /// * `limit` - Maximum number of receipts to return
    /// * `cursor` - Optional starting index for pagination (default: 0)
    ///
    /// # Returns
    /// * `Vec<Receipt>` - Vector of receipts matching the deal_id
    ///
    /// # Requirements
    /// * Returns receipts matching deal_id (Requirement 9.1)
    /// * Supports pagination (Requirements 9.2, 9.4, 9.5)
    /// * Returns receipts in deterministic order (Requirement 9.3)
    /// * No authorization required (public read)
    pub fn list_receipts_by_deal(
        env: soroban_sdk::Env,
        deal_id: String,
        limit: u32,
        cursor: Option<u32>,
    ) -> soroban_sdk::Vec<Receipt> {
        env.extend_instance_ttl();

        use soroban_sdk::Vec;

        let mut results = Vec::new(&env);

        // Get total count of receipts for this deal
        let deal_count_key = StorageKey::DealCount(deal_id.clone());
        let total_count: u32 = env.get_persistent(&deal_count_key).unwrap_or(0);

        // Calculate start index from cursor (default 0)
        let start_index = cursor.unwrap_or(0);

        // Calculate end index (start + limit, capped at total_count)
        let end_index = core::cmp::min(start_index + limit, total_count);

        // Iterate through deal index to load receipts
        for index in start_index..end_index {
            let deal_index_key = StorageKey::DealIndex(deal_id.clone(), index);

            // Load tx_id from deal index
            if let Some(tx_id) = env.get_persistent::<StorageKey, BytesN<32>>(&deal_index_key) {
                // Load receipt for this tx_id
                if let Some(receipt) =
                    env.get_persistent::<StorageKey, Receipt>(&StorageKey::Receipt(tx_id))
                {
                    results.push_back(receipt);
                }
            }
        }

        results
    }

    /// List receipts for a specific user with pagination
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `user` - The user address (from or to)
    /// * `limit` - Maximum number of receipts to return
    /// * `cursor` - Optional starting index for pagination
    ///
    /// # Returns
    /// A vector of receipts for the user, starting from cursor (or 0) up to limit
    pub fn list_receipts_by_user(
        env: soroban_sdk::Env,
        user: Address,
        limit: u32,
        cursor: Option<u32>,
    ) -> soroban_sdk::Vec<Receipt> {
        env.extend_instance_ttl();

        use soroban_sdk::Vec;

        let mut results = Vec::new(&env);

        let user_count_key = StorageKey::UserCount(user.clone());
        let total_count: u32 = env.get_persistent(&user_count_key).unwrap_or(0);

        let start_index = cursor.unwrap_or(0);
        let end_index = core::cmp::min(start_index + limit, total_count);

        for index in start_index..end_index {
            let user_index_key = StorageKey::UserIndex(user.clone(), index);

            if let Some(tx_id) = env.get_persistent::<StorageKey, BytesN<32>>(&user_index_key) {
                if let Some(receipt) =
                    env.get_persistent::<StorageKey, Receipt>(&StorageKey::Receipt(tx_id))
                {
                    results.push_back(receipt);
                }
            }
        }

        results
    }
}

#[contractimpl]
impl Pausable for TransactionReceiptContract {
    fn pause(env: soroban_sdk::Env, admin: Address) -> Result<(), PausableError> {
        if require_admin(&env, &admin).is_err() {
            return Err(PausableError::NotAuthorized);
        }
        env.storage().instance().set(&StorageKey::Paused, &true);
        env.events().publish(
            (Symbol::new(&env, "Pausable"), Symbol::new(&env, "pause")),
            (),
        );
        Ok(())
    }

    fn unpause(env: soroban_sdk::Env, admin: Address) -> Result<(), PausableError> {
        if require_admin(&env, &admin).is_err() {
            return Err(PausableError::NotAuthorized);
        }
        env.storage().instance().set(&StorageKey::Paused, &false);
        env.events().publish(
            (Symbol::new(&env, "Pausable"), Symbol::new(&env, "unpause")),
            (),
        );
        Ok(())
    }

    fn is_paused(env: soroban_sdk::Env) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&StorageKey::Paused)
            .unwrap_or(false)
    }
}

/// Helper function to verify that the caller is the admin
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The address attempting the operation
///
/// # Returns
/// * `Ok(())` - If the caller is the admin
/// * `Err(ContractError::NotAuthorized)` - If the caller is not the admin
fn require_admin(env: &soroban_sdk::Env, caller: &Address) -> Result<(), ContractError> {
    // Load admin address from storage
    let admin: Address = env
        .storage()
        .instance()
        .get(&StorageKey::Admin)
        .unwrap_or_else(|| panic!("Admin not initialized"));

    // Verify caller is admin
    if caller != &admin {
        return Err(ContractError::NotAuthorized);
    }

    Ok(())
}

/// Helper function to verify that the caller is the operator
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The address attempting the operation
///
/// # Returns
/// * `Ok(())` - If the caller is the operator
/// * `Err(ContractError::NotAuthorized)` - If the caller is not the operator
fn require_operator(env: &soroban_sdk::Env, caller: &Address) -> Result<(), ContractError> {
    // Load operator address from storage
    let operator: Address = env
        .storage()
        .instance()
        .get(&StorageKey::Operator)
        .unwrap_or_else(|| panic!("Operator not initialized"));

    // Verify caller is operator
    if caller != &operator {
        return Err(ContractError::NotAuthorized);
    }

    Ok(())
}

/// Helper function to verify that the contract is not paused
///
/// # Arguments
/// * `env` - The Soroban environment
///
/// # Returns
/// * `Ok(())` - If the contract is not paused
/// * `Err(ContractError::Paused)` - If the contract is paused
fn require_not_paused(env: &soroban_sdk::Env) -> Result<(), ContractError> {
    // Load paused state from storage (defaults to false if not set)
    let paused: bool = env
        .storage()
        .instance()
        .get(&StorageKey::Paused)
        .unwrap_or(false);

    // Return error if contract is paused
    if paused {
        return Err(ContractError::Paused);
    }

    Ok(())
}

/// Helper function to validate transaction type against allowed list
///
/// # Arguments
/// * `tx_type` - The transaction type to validate
///
/// # Returns
/// * `Ok(())` - If the transaction type is valid
/// * `Err(ContractError::InvalidTxType)` - If the transaction type is not in allowed list
fn validate_tx_type(env: &soroban_sdk::Env, tx_type: &Symbol) -> Result<(), ContractError> {
    use soroban_sdk::xdr::ToXdr;

    // Serialize the caller's Symbol and each allowed type with the SAME env and
    // compare like-for-like (see validate_external_ref for why raw-byte comparison
    // and a throwaway Env::default() are both wrong).
    let tx_type_bytes = tx_type.to_val().to_xdr(env);

    for allowed in ALLOWED_TX_TYPES.iter() {
        let allowed_bytes = Symbol::new(env, allowed).to_val().to_xdr(env);
        if tx_type_bytes == allowed_bytes {
            return Ok(());
        }
    }

    Err(ContractError::InvalidTxType)
}

/// Helper function to generate a deterministic transaction ID from external payment references
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `external_ref_source` - The payment source (must be in ALLOWED_SOURCES)
/// * `external_ref` - The external payment reference string
///
/// # Returns
/// * `Ok(BytesN<32>)` - SHA-256 hash of the canonical external reference string
/// * `Err(ContractError)` - If validation fails
///
/// # Validation Rules
/// * external_ref_source must be in ALLOWED_SOURCES (case-insensitive)
/// * external_ref must not be empty after trimming
/// * external_ref must not contain pipe character (|)
/// * external_ref must not exceed 256 characters
///
/// # Canonical Format
/// The canonical string format is: "v1|source=<lowercased_trimmed_source>|ref=<trimmed_ref>"
fn generate_tx_id(
    env: &soroban_sdk::Env,
    external_ref_source: &Symbol,
    external_ref: &String,
) -> Result<BytesN<32>, ContractError> {
    use soroban_sdk::Bytes;

    let (source_lower, ref_trimmed) =
        validate_and_normalize_ref(env, external_ref_source, external_ref)?;

    // Build canonical string "v1|source=<lowercased_trimmed_source>|ref=<trimmed_ref>" and hash it
    let mut combined = Bytes::new(env);
    combined.extend_from_slice(b"v1|source=");
    combined.extend_from_slice(source_lower.as_bytes());
    combined.extend_from_slice(b"|ref=");
    combined.append(&ref_trimmed);

    let hash = env.crypto().sha256(&combined);
    Ok(hash.into())
}

pub mod immutability_properties;

#[cfg(test)]
mod ttl_tests;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test;
#[cfg(test)]
mod tests;
// mod integration_tests;
// mod test;
// mod tests;
