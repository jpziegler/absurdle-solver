use std::error::Error;
use std::fs;
use std::time::Instant;
use itertools::all;
use rayon::prelude::*;
use serde::Deserialize;
use serde_json;
use clap::Parser;
use bitvec::prelude::*;

/// App to find solutions to the word game Absurdle
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of permutations to check
    #[arg(short, long, default_value_t = 2)]
    permutations: usize,
}

#[derive(Deserialize)]
struct WordLists {
    guesses: Vec<String>,
    solutions: Vec<String>,
    #[allow(dead_code)]
    winners: Vec<String>,
}

const NUM_BUCKETS:usize = 3 * 3 * 3 * 3 * 3; // 243

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
        if pattern[i] == 0 { // Not already green
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

fn intersect_size(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());
    let mut total:usize = 0;
    for (val_a, val_b) in a.iter().zip(b.iter()) {
        total += (val_a & val_b).count_ones() as usize;
    }
    total
}

fn intersect_size_bounded(a: &[u8], b: &[u8], bound:usize) -> usize {
    assert_eq!(a.len(), b.len());
    let mut total:usize = 0;
    for (val_a, val_b) in a.iter().zip(b.iter()) {
        total += (val_a & val_b).count_ones() as usize;
        if total > bound {
            break;
        }
    }
    total
}

fn intersect(a: &mut [u8], b: &[u8]) {
    assert_eq!(a.len(), b.len());
    for (val_a, val_b) in a.iter_mut().zip(b.iter()) {
        *val_a &= *val_b;
    }
}

fn find_best_bucket<'a>(
    current_set: &Box<[u8]>,
    buckets: &'a Vec<Box<[u8]>>,
) -> Option<&'a Box<[u8]>> {
    // Find the best bucket based on intersection size and tie-breaker score,
    // without allocating vectors for every single intersection.
    buckets
        .iter()
        .enumerate()
        // First, efficiently find the best bucket by calculating only the *size*
        // of each potential intersection, avoiding costly vector allocations.
        .map(|(hint, bucket)| {
            let size = intersect_size(bucket, &current_set);
            (bucket, size, hint)
        })
        .max_by(|&(_, size1, hint1), &(_, size2, hint2)| {
            // We want the bucket that results in the largest set of remaining words.
            // If there's a tie, we use the tie-breaker score to choose the minimum.
            size1.cmp(&size2)
                .then(compute_tie_breaker(hint2 as u32).cmp(&compute_tie_breaker(hint1 as u32)))
        })
        .map(|(bucket, _, _)| bucket)
}

fn find_best_bucket_bounded<'a>(
    current_set: &Box<[u8]>,
    buckets: &'a Vec<Box<[u8]>>,
    bound: usize,
) -> Option<&'a Box<[u8]>> {
    let mut max_item = None;

    for (hint, bucket) in buckets.iter().enumerate() {
        let size = intersect_size_bounded(bucket, &current_set, bound);
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
                    compute_tie_breaker(max_hint as u32)
                        .cmp(&compute_tie_breaker(hint as u32))
                });

                if ord == std::cmp::Ordering::Greater {
                    max_item = Some(current_item);
                }
            }
        }
    }

    max_item.map(|(bucket, _, _)| bucket)
}

// fn apply_guess<'a>(
//     current_set: &Box<[u8]>,
//     buckets: &'a Vec<Box<[u8]>>,
// ) -> Option<(Box<[u8]>, usize)> {
//     // Find the best bucket based on intersection size and tie-breaker score,
//     // without allocating vectors for every single intersection.
//     buckets
//         .iter()
//         .enumerate()
//         // First, efficiently find the best bucket by calculating only the *size*
//         // of each potential intersection, avoiding costly vector allocations.
//         .map(|(hint, bucket)| {
//             intersect(bucket, &current_set);
//             let size = bucket_size(&intersection);
//             (intersection, size, hint)
//         })
//         .max_by(|&(_, size1, hint1), &(_, size2, hint2)| {
//             // We want the bucket that results in the largest set of remaining words.
//             // If there's a tie, we use the tie-breaker score to choose the minimum.
//             size1.cmp(&size2)
//                 .then(compute_tie_breaker(hint2 as u32).cmp(&compute_tie_breaker(hint1 as u32)))
//         })
//         .map(|(intersection, size, _)| (intersection, size))
// }

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
    let mut solutions: Vec<String> = wls.solutions;
    solutions.sort();
    solutions.dedup();
    let mut guesses: Vec<String> = solutions.clone();
    guesses.extend(wls.guesses);
    guesses.sort();
    guesses.dedup();
    // let mut winners: Vec<Vec<String>> = wls.winners;
    // winners.sort();
    // winners.dedup();
    // for guess in guesses[210..220].iter() {
    //     println!("{}", guess);
    // }

    println!("Computing hints...");
    // let all_the_buckets: Vec<(String,Vec<Vec<String>>)> = guesses
    let all_the_buckets: Vec<(String,Vec<Box<[u8]>>)> = guesses
        .par_iter()
        .map(|guess| {
            let mut buckets = vec![vec![0; (solutions.len() + 8-1)/8].into_boxed_slice(); NUM_BUCKETS];
            assert!(NUM_BUCKETS == buckets.len());
            solutions
                .iter()
                .enumerate()
                .for_each(|(i, word)|{
                    if word != guess {
                        let hint = compute_wordle_hint(word, guess);
                        let bucket: &mut BitSlice<u8,Lsb0> = buckets[hint as usize].view_bits_mut();
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
    // let guesses_truncated: Vec<String> = ["ourie", "setal", "hansa", "tyler", "ohias", "panax", "token", "hairy", "scamp"].iter().map(|s| s.to_string()).collect();

    let solution_bucket: Box<[u8]> = vec![0xff; (solutions.len() + 8-1)/8].into_boxed_slice();
    
    // let find_winners_3 = || {
    //     println!("Finding winners for permutations of length 3...");
    //     let winners: Vec<Vec<String>> = all_the_buckets //[210..220]
    //         .par_iter()
    //         .enumerate()
    //         .flat_map(|(i, (g1, buckets1))| {
    //             let mut inner_winners = Vec::new();
    //             for (g2, buckets2) in &all_the_buckets {
    //                 if let Some((best_bucket, best_bucket_sz)) = find_best_bucket_bounded(&solution_bucket, &buckets1, NUM_BUCKETS) {
    //                     if best_bucket_sz > NUM_BUCKETS {
    //                         println!("Skipping {} due to too many possibilities. ({}/{})", g1, i + 1, guesses.len());
    //                     } else {
    //                         println!("Analyzing {} ({}/{})", g1, i + 1, guesses.len());
    //                         let mut my_bucket = solution_bucket.clone();
    //                         intersect(&mut my_bucket, &best_bucket);

    //                         for (gfinal, buckets_final) in &all_the_buckets {
    //                             if let Some((_, final_word_vec_sz)) = find_best_bucket_bounded(&my_bucket, &buckets_final, 1) {
    //                                 if final_word_vec_sz == 1 {
    //                                     // apply_guess(gfinal, &solutions_final, &all_the_buckets);
    //                                     let winner = vec![g1.clone(), gfinal.clone()];
    //                                     println!("Winner: {:?}", winner);
    //                                     inner_winners.push(winner);
    //                                 }
    //                             }
    //                         }
    //                     }
    //                 }
    //             }
    //             inner_winners
    //         })
    //         .collect();
    //     winners
    // };

    let find_winners_3 = || {
        println!("Finding winners for permutations of length 3...");
        let winners: Vec<Vec<String>> = all_the_buckets //[210..220]
            .par_iter()
            .enumerate()
            .flat_map(|(i, (g1, buckets1))| {
                let mut inner_winners = Vec::new();
                
                if let Some(best_bucket) = find_best_bucket_bounded(&solution_bucket, &buckets1, NUM_BUCKETS) {
                    println!("Analyzing {} ({}/{})", g1, i + 1, guesses.len());
                    let mut my_bucket = solution_bucket.clone();
                    intersect(&mut my_bucket, &best_bucket);
                    for (gfinal, buckets_final) in &all_the_buckets {
                        if let Some(_) = find_best_bucket_bounded(&my_bucket, &buckets_final, 1) {
                            // apply_guess(gfinal, &solutions_final, &all_the_buckets);
                            let winner = vec![g1.clone(), gfinal.clone()];
                            println!("Winner: {:?}", winner);
                            inner_winners.push(winner);
                        }
                    }
                }
                inner_winners
            })
            .collect();
        winners
    };

    let find_winners_2 = || {
        println!("Finding winners for permutations of length 3...");
        let winners: Vec<Vec<String>> = all_the_buckets //[210..220]
            .par_iter()
            .enumerate()
            .flat_map(|(i, (g1, buckets1))| {
                let mut inner_winners = Vec::new();
                if let Some(best_bucket) = find_best_bucket_bounded(&solution_bucket, &buckets1, NUM_BUCKETS) {
                    println!("Analyzing {} ({}/{})", g1, i + 1, guesses.len());
                    let mut my_bucket = solution_bucket.clone();
                    intersect(&mut my_bucket, &best_bucket);
                    for (gfinal, buckets_final) in &all_the_buckets {
                        if let Some(_) = find_best_bucket_bounded(&my_bucket, &buckets_final, 1) {
                            // apply_guess(gfinal, &solutions_final, &all_the_buckets);
                            let winner = vec![g1.clone(), gfinal.clone()];
                            println!("Winner: {:?}", winner);
                            inner_winners.push(winner);
                        }
                    }
                }
                inner_winners
            })
            .collect();
        winners
    };
    // let find_winners_2 = || {
    //     println!("Finding winners for permutations of length 2...");
    //     let winners: Vec<Vec<String>> = all_the_buckets
    //         .par_iter()
    //         .enumerate()
    //         .flat_map(|(i, (g1, buckets1))| {
    //             let mut inner_winners = Vec::new();
    //             let (best_bucket, best_bucket_size) = find_best_bucket(&solutions, &buckets1);
    //             if best_bucket_size > NUM_BUCKETS {
    //                 // println!("Skipping {} due to too many possibilities. ({}/{})", g1, i + 1, guesses.len());
    //             } else {
    //                 let solutions_final = intersect_sorted_vecs(best_bucket, &solutions);
    //                 println!("Analyzing {} ({}/{})", g1, i + 1, guesses.len());
    //                 for (gfinal, buckets_final) in &all_the_buckets {
    //                     let (best_bucket, best_bucket_size) = find_best_bucket(&solutions_final, &buckets_final);
    //                     if best_bucket_size == 1 {
    //                         let final_word_vec = intersect_sorted_vecs(best_bucket, &solutions_final);
    //                         let final_word = final_word_vec.iter().next().unwrap();
    //                         let winner = vec![g1.clone(), gfinal.clone(), final_word.clone()];
    //                         println!("Winner: {:?}", winner);
    //                         inner_winners.push(winner);
    //                     }
    //                 }
    //             }
    //             inner_winners
    //         })
    //         .collect();
    //     winners
    // };

    let winners = if args.permutations == 3 {
        find_winners_3()
    } else {
        find_winners_2()
    };

    for winner in winners {
            println!("Winner: {:?}", winner);
    }
    println!("\nResults computed in {:?}", start_time.elapsed());

    Ok(())
}
