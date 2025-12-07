# Absurdle Solver

Program that finds all minimal solutions to 
[Absurdle](https://qntm.org/files/absurdle/absurdle.html), an adversarial version of
[Wordle](https://www.nytimes.com/games/wordle/index.html).

Written in Rust using [Rayon](https://docs.rs/rayon/latest/rayon/) for parallelism.

## Usage

To prove that there are no 3-word solutions. (Very fast)
```
cargo run --release -- --permutations 2
```
To generate all 4-word solutions. (Roughly 8 hours on an Apple M3 Max)
```
cargo run --release -- --permutations 3
```
