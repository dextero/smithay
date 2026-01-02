---
description: Run standard Rust checks (fmt, clippy, test)
---
// turbo-all

1. Check formatting
2. cargo fmt --all -- --check

3. Run clippy
4. cargo clippy --all-targets --all-features -- -D warnings

5. Run all tests
6. cargo test --all-features
