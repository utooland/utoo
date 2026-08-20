/*
Copyright (c) 2017 The swc Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
*/

#![recursion_limit = "2048"]
//#![deny(clippy::all)]
#![feature(arbitrary_self_types)]
#![feature(arbitrary_self_types_pointers)]
#![allow(unexpected_cfgs)]

#[macro_use]
extern crate napi_derive;

pub mod lockfile;
pub mod pack_api;
pub mod util;
pub mod worker_pool;

#[cfg(not(any(feature = "__internal_dhat-heap", feature = "__internal_dhat-ad-hoc")))]
#[global_allocator]
static ALLOC: turbo_tasks_malloc::TurboMalloc = turbo_tasks_malloc::TurboMalloc;

#[cfg(feature = "__internal_dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(not(target_arch = "wasm32"))]
#[napi_derive::module_init]
fn init() {
    use std::{
        cell::RefCell,
        panic::{set_hook, take_hook},
        time::{Duration, Instant},
    };

    use tokio::runtime::Builder;
    use turbo_tasks::panic_hooks::handle_panic;
    use turbo_tasks_malloc::TurboMalloc;

    let prev_hook = take_hook();
    set_hook(Box::new(move |info| {
        handle_panic(info);
        prev_hook(info);
    }));

    thread_local! {
        static LAST_SWC_ATOM_GC_TIME: RefCell<Option<Instant>> = const { RefCell::new(None) };
    }

    let rt = Builder::new_multi_thread()
        .enable_all()
        .on_thread_stop(|| {
            TurboMalloc::thread_stop();
        })
        .on_thread_park(|| {
            LAST_SWC_ATOM_GC_TIME.with_borrow_mut(|cell| {
                if cell.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
                    swc_core::ecma::atoms::hstr::global_atom_store_gc();
                    *cell = Some(Instant::now());
                }
            });
        })
        .disable_lifo_slot()
        .build()
        .unwrap();
    napi::bindgen_prelude::create_custom_tokio_runtime(rt);
}
