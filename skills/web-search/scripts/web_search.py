#!/usr/bin/env python3
"""Web search using DuckDuckGo."""

import json
import sys

from duckduckgo_search import DDGS


def main():
    if len(sys.argv) < 2:
        print(json.dumps({"error": "Usage: web_search.py <query> [num_results]"}))
        sys.exit(1)

    query = sys.argv[1]
    num = int(sys.argv[2]) if len(sys.argv) > 2 else 10

    try:
        results = []
        with DDGS() as ddgs:
            for r in ddgs.text(query, max_results=num):
                results.append({
                    "title": r.get("title"),
                    "url": r.get("href"),
                    "snippet": r.get("body"),
                })
    except Exception as e:
        print(json.dumps({"error": f"Search failed: {e}"}))
        sys.exit(1)

    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
