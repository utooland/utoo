//! `Send`/`Sync` shim that is `Send`/`Sync` on native targets and
//! no-op on `wasm32`. Lets the resolver share a single trait surface
//! across multi-thread tokio (where futures must be `Send` to be
//! `tokio::spawn`-able) and wasm-bindgen (where `JsFuture` is `!Send`
//! and tasks run via `wasm_bindgen_futures::spawn_local`).

/// `Send` on native, no-op on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// `Sync` on native, no-op on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync> MaybeSync for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSync for T {}
