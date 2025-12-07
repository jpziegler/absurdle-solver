use bitvec::prelude::*;
use clap::Parser;
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::time::Instant;

/// App to find solutions to the word game Absurdle
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of permutations to check
    #[arg(short, long, default_value_t = 2)]
    permutations: usize,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct WordLists {
    N: HashMap<String, String>,
    I: HashMap<String, String>,
}

type Wd = u64;
const NUM_BUCKETS: usize = 3 * 3 * 3 * 3 * 3; // 243

fn compute_colors(word: &str, guess: &str) -> [u32; 5] {
    let mut pattern = [0u32; 5];
    let mut word_chars: Vec<Option<char>> = word.chars().map(Some).collect();
    let guess_chars: Vec<char> = guess.chars().collect();

    // First pass for GREEN matches
    for i in 0..5 {
        if Some(guess_chars[i]) == word_chars[i] {
            pattern[i] = 2; // Green
            word_chars[i] = None; // Mark as used
        }
    }

    // Second pass for YELLOW matches
    for i in 0..5 {
        if pattern[i] == 0 {
            // Not already green
            if let Some(pos) = word_chars.iter().position(|&c| c == Some(guess_chars[i])) {
                pattern[i] = 1; // Yellow
                word_chars[pos] = None; // Mark as used
            }
        }
    }
    pattern
}

fn compute_wordle_hint(word: &str, guess: &str) -> u32 {
    let pattern = compute_colors(word, guess);
    // Convert base-3 hint array to a single u32 number
    pattern.iter().fold(0, |acc, &d| acc * 3 + d)
}

fn hint_to_pattern(hint: u32) -> [u32; 5] {
    let mut pattern = [0; 5];
    let mut n = hint;
    for i in 0..5 {
        pattern[4 - i] = n % 3;
        n /= 3;
    }
    pattern
}

fn compute_tie_breaker(hint: u32) -> u64 {
    let pattern = hint_to_pattern(hint);
    let mut score = 0u64;
    for (i, &tile) in pattern.iter().enumerate() {
        let tile_value = tile as u64;

        // This part of the score is dominant and rewards more informative tiles.
        // 10^7 for Green, 10^6 for Yellow, 10^5 for Gray.
        let term1 = 10u64.pow(5 + tile_value as u32);

        // This part of the score is a secondary tie-breaker, rewarding tiles
        // that appear earlier in the word.
        let term2 = tile_value * 10u64.pow((4 - i) as u32);

        score += term1 + term2;
    }
    score
}

// This function is the "hot loop" - the most execution time is spent here.
pub fn intersect_size(a: &[Wd], b: &[Wd]) -> usize {
    a.iter()
        .zip(b.iter())
        .map(|(val_a, val_b)| (val_a & val_b).count_ones() as usize)
        .sum()
}

fn intersect(a: &[Wd], b: &[Wd]) -> Box<[Wd]> {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(val_a, val_b)| val_a & val_b)
        .collect::<Vec<Wd>>()
        .into_boxed_slice()
}

fn find_initial_bucket<'a>(buckets: &'a Vec<Box<[Wd]>>) -> Option<&'a Box<[Wd]>> {
    // Find the best bucket based on bucket size and tie-breaker score.
    buckets
        .iter()
        .enumerate()
        // First, efficiently find the best bucket by calculating only the *size*
        .map(|(hint, bucket)| {
            let size: usize = bucket.iter().map(|val| val.count_ones() as usize).sum();
            (bucket, size, hint)
        })
        .max_by(|&(_, size1, hint1), &(_, size2, hint2)| {
            // We want the bucket that results in the largest set of remaining words.
            // If there's a tie, we use the tie-breaker score to choose the minimum.
            size1
                .cmp(&size2)
                .then(compute_tie_breaker(hint2 as u32).cmp(&compute_tie_breaker(hint1 as u32)))
        })
        .map(|(bucket, _, _)| bucket)
}

fn find_best_bucket_bounded<'a>(
    current_set: &Box<[Wd]>,
    buckets: &'a Vec<Box<[Wd]>>,
    bound: usize,
) -> Option<&'a Box<[Wd]>> {
    let mut max_item = None;

    for (hint, bucket) in buckets.iter().enumerate() {
        // let size = intersect_size_bounded(bucket, &current_set, bound);
        let size = intersect_size(bucket, &current_set);
        if size > bound {
            return None;
        }

        let current_item = (bucket, size, hint);
        match max_item {
            None => {
                max_item = Some(current_item);
            }
            Some((_, max_size, max_hint)) => {
                let ord = size.cmp(&max_size).then_with(|| {
                    compute_tie_breaker(max_hint as u32).cmp(&compute_tie_breaker(hint as u32))
                });

                if ord == std::cmp::Ordering::Greater {
                    max_item = Some(current_item);
                }
            }
        }
    }

    max_item.map(|(bucket, _, _)| bucket)
}

fn write_winners(winners: &Vec<Vec<String>>) -> Result<(), std::io::Error> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("winners.txt")?;
    for winner in winners {
        writeln!(f, "{}", winner.join(","))?;
    }
    Ok(())
}

fn extract_wordlist(m: &HashMap<String, String>) -> Vec<String> {
    m.iter().fold(Vec::new(), |mut acc: Vec<String>, (prefix, suffixes)| {
        assert!(prefix.len() == 2, "Wordlist data invalid. Prefix string must be exactly 2 characters.");
        acc.extend(
            suffixes.chars()
                .collect::<Vec<char>>() // Collect chars into a Vec<char> to use .chunks()
                .chunks(3)              // Create an iterator of chunks of size n
                .map(|chunk| {
                    assert!(chunk.len() == 3, "Wordlist data invalid. Suffix string must have a multiple of 3 characters.");
                    chunk.iter().collect::<String>()
                }) // Convert each chunk back to a String
                .collect::<Vec<String>>()
                .iter() // Collect all resulting strings into a Vec
                .map(|suffix| prefix.to_owned() + suffix)
        );
        acc
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if args.permutations != 2 && args.permutations != 3 {
        panic!("permutations must be 2 or 3");
    }

    let pattern = compute_colors("clash", "batch");
    let hint = compute_wordle_hint("clash", "batch");
    assert!(pattern == hint_to_pattern(hint));

    let mut start_time = Instant::now();

    let file = "res/absurdle.json";

    println!("Reading words from: {:?}", file);
    let file_content = fs::read_to_string(file)?;
    let wls: WordLists = serde_json::from_str(file_content.as_str())?;
    let mut solutions = extract_wordlist(&wls.N);
    solutions.sort();
    solutions.dedup();
    let mut guesses: Vec<String> = solutions.clone();
    guesses.extend(extract_wordlist(&wls.I));
    guesses.sort();
    guesses.dedup();

    // Size of the bucket when encoded as a bitset
    let bucket_sz = (solutions.len() + (Wd::BITS as usize) - 1) / (Wd::BITS as usize);

    println!("Computing hints...");
    let all_the_buckets: Vec<(String, Vec<Box<[Wd]>>)> = guesses
        .par_iter()
        .map(|guess| {
            let mut buckets = vec![vec![0; bucket_sz].into_boxed_slice(); NUM_BUCKETS];
            assert!(NUM_BUCKETS == buckets.len());
            solutions.iter().enumerate().for_each(|(i, word)| {
                if word != guess {
                    let hint = compute_wordle_hint(word, guess);
                    let bucket: &mut BitSlice<Wd, Lsb0> = buckets[hint as usize].view_bits_mut();
                    bucket.set(i, true);
                }
            });
            (guess.clone(), buckets)
        })
        .collect();

    println!("\nWords read in {:?}", start_time.elapsed());
    start_time = Instant::now();

    assert!(guesses.len() == all_the_buckets.len());

    println!("Number of guesses to check: {}", guesses.len());

    let find_winners_3 = |start: usize, end: usize| {
        println!("Finding winners for permutations of length 3...");
        let winners: Vec<Vec<String>> = all_the_buckets[start..end]
            .par_iter()
            .enumerate()
            .flat_map(|(i, (g1, buckets1))| {
                let mut inner_winners = Vec::new();
                if let Some(solutions2) = find_initial_bucket(&buckets1) {
                    println!("Analyzing {} ({}/{})", g1, i + 1, guesses.len());
                    for (g2, buckets2) in &all_the_buckets {
                        if let Some(best_bucket) =
                            find_best_bucket_bounded(&solutions2, &buckets2, NUM_BUCKETS)
                        {
                            let solutions_final = intersect(&solutions2, &best_bucket);
                            for (gfinal, buckets_final) in &all_the_buckets {
                                if let Some(final_bucket) =
                                    find_best_bucket_bounded(&solutions_final, &buckets_final, 1)
                                {
                                    assert!(intersect_size(&solutions_final, &final_bucket) == 1);
                                    let solution = intersect(&solutions_final, &final_bucket);
                                    let final_bucket_bitslice: &BitSlice<Wd, Lsb0> =
                                        solution.view_bits();
                                    let solution =
                                        &solutions[final_bucket_bitslice.first_one().unwrap()];
                                    let winner = vec![
                                        g1.clone(),
                                        g2.clone(),
                                        gfinal.clone(),
                                        solution.clone(),
                                    ];
                                    println!("Winner: {}", winner.join(","));
                                    inner_winners.push(winner);
                                }
                            }
                        }
                    }
                }
                inner_winners
            })
            .collect();
        winners
    };

    let find_winners_2 = || {
        let solution_bucket: Box<[Wd]> = vec![!0; bucket_sz].into_boxed_slice();
        println!("Finding winners for permutations of length 2...");
        let winners: Vec<Vec<String>> = all_the_buckets
            .par_iter()
            .enumerate()
            .flat_map(|(i, (g1, buckets1))| {
                let mut inner_winners = Vec::new();
                if let Some(best_bucket) =
                    find_best_bucket_bounded(&solution_bucket, &buckets1, NUM_BUCKETS)
                {
                    println!("Analyzing {} ({}/{})", g1, i + 1, guesses.len());
                    let solutions_final = intersect(&solution_bucket, &best_bucket);
                    for (gfinal, buckets_final) in &all_the_buckets {
                        if let Some(_) =
                            find_best_bucket_bounded(&solutions_final, &buckets_final, 1)
                        {
                            let winner = vec![g1.clone(), gfinal.clone()];
                            println!("Winner: {}", winner.join(","));
                            inner_winners.push(winner);
                        }
                    }
                }
                inner_winners
            })
            .collect();
        winners
    };

    let mut winners;
    if args.permutations == 3 { // Long run
        let step = 100000; // Done a chunk at a time to allow cancel/resume
        for i in (0..guesses.len()).step_by(step) {
            let end = std::cmp::min(guesses.len(), i + step);
            winners = find_winners_3(i, end);
            write_winners(&winners).unwrap();
            println!("Completed {} through {}", i, end);
        }
    } else {
        winners = find_winners_2();
        write_winners(&winners).unwrap();
    };

    println!("\nResults computed in {:?}", start_time.elapsed());

    Ok(())
}
