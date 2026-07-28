#![no_std]
//! Shared in-contract pausable primitives.
//!
//! These are *linked* helpers, not cross-contract calls: a contract that guards a
//! path calls into this module directly, so the guard costs one instance-storage
//! read/write and never leaves the contract. That is deliberate — routing the check
//! through the deployable `soroban_pausable` contract would put an external call
//! inside the guard itself, which is both more gas and a weaker security model.
//!
//! This is a **library-only** crate (`crate-type = ["rlib"]`, no `#[contract]`). The
//! primitives deliberately do not live alongside the `soroban_pausable` contract:
//! the workspace release profile uses fat LTO, so linking a crate that carries
//! `#[contractimpl]` entry points pulls those exports into the dependent contract's
//! WASM and fails the build with `Linking globals named 'init': symbol multiply
//! defined!`. Keep this crate free of contract entry points.

use soroban_sdk::{contracterror, Address, Env};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PausableError {
    Paused = 3001,
    NotAuthorized = 3002,
}

pub trait Pausable {
    /// Pause the contract. Only an authorized admin should be able to trigger this.
    fn pause(env: Env, admin: Address) -> Result<(), PausableError>;

    /// Unpause the contract. Only an authorized admin should be able to trigger this.
    fn unpause(env: Env, admin: Address) -> Result<(), PausableError>;

    /// Check if the contract is paused.
    fn is_paused(env: Env) -> bool;
}
