//! Cross-target `Send`/`Sync` shims.
//!
//! On native targets `MaybeSend` is `Send` and `MaybeSync` is `Sync`.
//! On `wasm32-unknown-unknown` they are vacuous marker traits
//! implemented for every type — wasm-bindgen futures use
//! `Rc<RefCell<...>>` internally and cannot satisfy real `Send`/`Sync`.
//!
//! Using these in trait bounds lets the same trait surface compile
//! against both native (where `tokio::spawn` requires `Send`) and the
//! single-threaded wasm target.

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: ?Sized + Send> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: ?Sized + Sync> MaybeSync for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSync for T {}
