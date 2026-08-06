//! Emits an `app_bootstrap` payload as JSON on stdout.
//!
//! This is what `tests/wire_shape.rs` compares the golden fixture against, and
//! what regenerates it:
//!
//! ```text
//! LOTUS_BLESS_FIXTURE=1 cargo test --test wire_shape
//! ```
//!
//! It builds the payload by hand rather than by connecting a provider, so it
//! stays deterministic and needs no database.

fn main() {
    println!("{}", lotus_app_lib::wire_fixture_json());
}
