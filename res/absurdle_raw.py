import json

with open("absurdle_raw.json") as f:
    j = json.load(f)
N = j["N"]
I = j["I"]

words_n = []
words_i = []
for (set, words) in [(N,words_n),(I, words_i)]:
    for prefix in set:
        suffixes = set[prefix]
        words += [(prefix+suffixes[i:i+3]).lower() for i in range(0, len(suffixes), 3)]

with open("absurdle.json", "w") as f:
    json.dump({"guesses": words_i, "solutions": words_n}, f)