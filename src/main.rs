use std::error::Error;
use std::fs;
use std::time::Instant;
use rayon::prelude::*;
use serde::Deserialize;
use serde_json;
use clap::Parser;

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

fn count_intersection_size<T: Ord>(vec1: &[T], vec2: &[T]) -> usize {
    let mut count = 0;
    let mut i = 0;
    let mut j = 0;

    while i < vec1.len() && j < vec2.len() {
        if vec1[i] < vec2[j] {
            i += 1;
        } else if vec1[i] > vec2[j] {
            j += 1;
        } else {
            count += 1;
            i += 1;
            j += 1;
        }
    }
    count
}

fn intersect_sorted_vecs<T>(vec1: &[T], vec2: &[T]) -> Vec<T>
where
    T: Ord + Clone,
{
    let mut result = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < vec1.len() && j < vec2.len() {
        if vec1[i] < vec2[j] {
            i += 1;
        } else if vec1[i] > vec2[j] {
            j += 1;
        } else {
            result.push(vec1[i].clone());
            i += 1;
            j += 1;
        }
    }
    result
}

fn find_best_bucket<'a>(
    current_set: &Vec<String>,
    buckets: &'a Vec<Vec<String>>,
) -> (&'a Vec<String>, usize) {
    // Find the best bucket based on intersection size and tie-breaker score,
    // without allocating vectors for every single intersection.
    buckets
        .iter()
        .enumerate()
        // First, efficiently find the best bucket by calculating only the *size*
        // of each potential intersection, avoiding costly vector allocations.
        .map(|(hint, bucket)| (bucket, count_intersection_size(bucket, current_set), hint))
        .max_by(|&(_, len1, hint1), &(_, len2, hint2)| {
            // We want the bucket that results in the largest set of remaining words.
            // If there's a tie, we use the tie-breaker score to choose the minimum.
            len1.cmp(&len2)
                .then(compute_tie_breaker(hint2 as u32).cmp(&compute_tie_breaker(hint1 as u32)))
        })
        .map(|(bucket, size, _)| (bucket, size))
        .unwrap()
}

fn count_intersection_size_bounded<T: Ord>(vec1: &[T], vec2: &[T]) -> usize {
    let mut count = 0;
    let mut i = 0;
    let mut j = 0;

    while i < vec1.len() && j < vec2.len() {
        if vec1[i] < vec2[j] {
            i += 1;
        } else if vec1[i] > vec2[j] {
            j += 1;
        } else {
            count += 1;
            if count > NUM_BUCKETS {
                return count;
            }
            i += 1;
            j += 1;
        }
    }
    count
}

fn find_best_bucket_bounded<'a>(
    current_set: &Vec<String>,
    buckets: &'a Vec<Vec<String>>,
) -> Option<(&'a Vec<String>, usize)> {
    let mut best_candidate: Option<(&'a Vec<String>, usize, u32)> = None;

    for (hint, bucket) in buckets.iter().enumerate() {
        let size = count_intersection_size_bounded(bucket, current_set);

        if size > NUM_BUCKETS {
            return None; // Condition met, stop iteration.
        }

        let is_better = if let Some((_, best_size, best_hint)) = best_candidate {
            match size.cmp(&best_size) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Equal => {
                    compute_tie_breaker(hint as u32) < compute_tie_breaker(best_hint)
                }
                std::cmp::Ordering::Less => false,
            }
        } else {
            true
        };

        if is_better {
            best_candidate = Some((bucket, size, hint as u32));
        }
    }

    best_candidate.map(|(bucket, size, _)| (bucket, size))
}

fn apply_guess(current_set: &Vec<String>, buckets: &Vec<Vec<String>>) -> Vec<String> {
    let best_bucket_with_size = find_best_bucket(current_set, buckets);

    // Now that we've identified the single best bucket, perform the actual
    // intersection operation exactly once.
    let (best_bucket, _) = best_bucket_with_size;
    intersect_sorted_vecs(best_bucket, current_set)
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
    let all_the_buckets: Vec<(String,Vec<Vec<String>>)> = guesses
        .par_iter()
        .map(|guess| {
            let mut buckets: Vec<Vec<String>> = vec![Vec::new(); NUM_BUCKETS];
            solutions.iter().for_each(|word| {
                if word != guess {
                    let hint = compute_wordle_hint(word, guess);
                    buckets[hint as usize].push(word.to_string());
                }
            });
            assert!(NUM_BUCKETS == buckets.len());
            (guess.clone(), buckets)
            })
        .collect();
    
    println!("\nWords read in {:?}", start_time.elapsed());
    start_time = Instant::now();

    assert!(guesses.len() == all_the_buckets.len());

    println!("Number of guesses to check: {}", guesses.len());
    // let guesses_truncated: Vec<String> = ["ourie", "setal", "hansa", "tyler", "ohias", "panax", "token", "hairy", "scamp"].iter().map(|s| s.to_string()).collect();

    let find_winners_3 = || {
        println!("Finding winners for permutations of length 3...");
        let winners: Vec<Vec<String>> = all_the_buckets //[210..220]
            .par_iter()
            .enumerate()
            .flat_map(|(i, (g1, buckets1))| {
                let mut inner_winners = Vec::new();
                let solutions1 = apply_guess(&solutions, buckets1);
                println!("Analyzing {} ({}/{})", g1, i + 1, guesses.len());
                for (g2, buckets2) in &all_the_buckets {
                    // let solutions_final = apply_guess(&solutions1, &buckets2);
                    let (best_bucket2, best_bucket2_sz) = find_best_bucket(&solutions1, &buckets2);
                    if best_bucket2_sz > NUM_BUCKETS {
                        println!("Skipping {} {} due to too many possibilities. ({}/{})", g1, g2, i + 1, guesses.len());
                    } else {
                        let solutions_final = intersect_sorted_vecs(best_bucket2, &solutions1);
                        for (gfinal, buckets_final) in &all_the_buckets {
                            let (best_bucket_final, best_bucket_final_sz) = find_best_bucket(&solutions_final, &buckets_final);
                            if best_bucket_final_sz == 1 {
                                let final_word_vec = intersect_sorted_vecs(best_bucket_final, &solutions_final);
                                // apply_guess(gfinal, &solutions_final, &all_the_buckets);
                                let final_word = final_word_vec.iter().next().unwrap();
                                let winner = vec![g1.clone(), g2.clone(), gfinal.clone(), final_word.clone()];
                                println!("Winner: {:?}", winner);
                                inner_winners.push(winner);
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
        println!("Finding winners for permutations of length 2...");
        let winners: Vec<Vec<String>> = all_the_buckets
            .par_iter()
            .enumerate()
            .flat_map(|(i, (g1, buckets1))| {
                let mut inner_winners = Vec::new();
                let (best_bucket, best_bucket_size) = find_best_bucket(&solutions, &buckets1);
                if best_bucket_size > NUM_BUCKETS {
                    // println!("Skipping {} due to too many possibilities. ({}/{})", g1, i + 1, guesses.len());
                } else {
                    let solutions_final = intersect_sorted_vecs(best_bucket, &solutions);
                    println!("Analyzing {} ({}/{})", g1, i + 1, guesses.len());
                    for (gfinal, buckets_final) in &all_the_buckets {
                        let (best_bucket, best_bucket_size) = find_best_bucket(&solutions_final, &buckets_final);
                        if best_bucket_size == 1 {
                            let final_word_vec = intersect_sorted_vecs(best_bucket, &solutions_final);
                            let final_word = final_word_vec.iter().next().unwrap();
                            let winner = vec![g1.clone(), gfinal.clone(), final_word.clone()];
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
