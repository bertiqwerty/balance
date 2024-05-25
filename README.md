# Balance
[![CI](https://github.com/bertiqwerty/balance/actions/workflows/rust.yml/badge.svg)](https://github.com/bertiqwerty/balance)
[![dependency status](https://deps.rs/repo/github/bertiqwerty/balance/status.svg)](https://deps.rs/repo/github/bertiqwerty/balance)

Simulate portfolio balance or backtest with or without rebalancing under https://www.bertiqwerty.com/balance/.
Alternatively, you can run Balance as a desktop app if you have the Rust and Cargo installed via
```
cargo install rebalance
rebalance
```

<sub>Created with [eframe](https://github.com/emilk/egui/tree/master/crates/eframe), a framework for writing apps using [egui](https://github.com/emilk/egui/).</sub>

## Development

### Build WASM

See https://github.com/emilk/eframe_template.

### Run locally

#### Native

```
cargo run --release
```

#### WASM

```
trunk serve --public-url /
```
