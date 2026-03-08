mod in_memory;
mod storage_trait;

#[cfg(feature = "rocksdb")]
mod rocksdb_impl;

pub use in_memory::InMemoryStorage;
pub use storage_trait::{Storage, StorageError};

#[cfg(feature = "rocksdb")]
pub use rocksdb_impl::RocksDBStorage;
