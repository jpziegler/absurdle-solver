from selenium import webdriver
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.by import By
from selenium.webdriver import ActionChains
import sys, time

# find_winners.py - tests the results of absurdle_solver against a mirror of the website.
# Exits with code 0 on success, >0 on failure.

driver = webdriver.Firefox()
# # To create a runnable mirror of the absurdle website for local testing:
# mkdir absurdle_mirror
# cd absurdle_mirror
# wget -r -p -k -np --convert-links --page-requisites --span-hosts https://qntm.org/files/absurdle/absurdle.html
# python -m http.server 8000
driver.get("http://localhost:8000/qntm.org/files/absurdle/absurdle.html")
assert "Absurdle" in driver.title
code = 0
with open("winners.txt") as f:
    for line in f:
        for word in line.split(','):
            ActionChains(driver)\
                .send_keys(word)\
                .send_keys(Keys.RETURN)\
                .perform()
        if "You guessed successfully in 4 guesses!" not in driver.page_source:
            print("Failed: " + line)
            code += 1
        driver.refresh()
driver.close()
sys.exit(code)