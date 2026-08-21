import os
import requests

base = "https://raw.githubusercontent.com/github/gitignore/refs/heads/main/"
urls = [
    "Global/Windows.gitignore",
    "Global/Linux.gitignore",
    "Global/macOS.gitignore",
    "Python.gitignore",
    "R.gitignore",
]

f = os.path.dirname(__file__)

with open(f + "/default.gitignore", "w") as outfile:    
    outfile.write(".pki\n.config\n\n")
    
    for url in urls:
        file = base + url
        response = requests.get(file)
        txt = response.text

        outfile.write(f"## Taken from {url}\n")
        outfile.write(txt + "\n")
