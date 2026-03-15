default: lint

lint: fmt check audit

fmt:
    cargo fmt

check:
    cargo clippy

audit:
    cargo audit

build:
    cargo build --release

install:
    cargo install --path .
