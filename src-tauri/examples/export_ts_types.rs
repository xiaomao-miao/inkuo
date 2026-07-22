//! Triggers `ts-rs` to walk every `#[derive(TS)] #[ts(export)]` type and
//! write the corresponding `.ts` files into `src/types/generated/`.
//!
//! Run with:
//!     cargo run --example export_ts_types
//!
//! Lives in `examples/` (not `src/bin/`) so `cargo run` (used by `tauri dev`)
//! has a single unambiguous binary to launch.
//!
//! The output directory is configured via `#[ts(export_to = "...")]` on each
//! type. Calling `ts_rs::TS::export_all()` on each type forces ts-rs to
//! perform its codegen pass; the side effect is the .ts file appearing on
//! disk.

mod crate_exports {
    pub use inkuo_lib::commands::FileEntry;
    pub use inkuo_lib::error::AppError;
}

fn main() {
    use ts_rs::TS;
    crate_exports::FileEntry::export_all().expect("failed to export FileEntry");
    crate_exports::AppError::export_all().expect("failed to export AppError");

    println!("ts-rs types exported");
}