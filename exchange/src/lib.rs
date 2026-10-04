//! S0 contract library. No network, durable acknowledgement or chain integration.
pub mod adapter;
pub mod codec;
pub mod policy;
pub type Result<T> = std::result::Result<T, &'static str>;

pub mod decision;

pub mod s2;
