import sys
import json

def main():
    args_file = sys.argv[1]
    with open(args_file) as f:
        params = json.load(f)
    print(params)

main()