# Absurdle Solver

Program that finds all minimal solutions to [Absurdle](https://qntm.org/files/absurdle/absurdle.html).

Written in Rust using [Rayon](https://docs.rs/rayon/latest/rayon/) for parallelism.

## Usage

To prove that there are no 3-word solutions. (Very fast)
```
cargo run --release -- --permutations 2
```
To generate all 4-word solutions. (Roughly 8 hours on an M3 mac)
```
cargo run --release -- --permutations 3
```
