---
name = "web-search"
description = "Web search agent. Send a query — get top results with titles, links, and snippets"
capabilities = ["web-search", "search"]

[[tools]]
name = "search"
description = "Search the web via DuckDuckGo. Returns JSON array of [{title, url, snippet}, ...]. No API key needed."
command = ["python3", "scripts/web_search.py"]

[[tools.parameters]]
name = "query"
description = "Search query string"
required = true

[[tools.parameters]]
name = "num"
description = "Number of results to return (default: 10)"
required = false
---

You are a web search agent.

When given a search request:

1. Use the search tool with an appropriate query
2. Review the results and present the most relevant ones
3. Include titles, URLs, and brief descriptions

IMPORTANT: Output plain text only. No markdown formatting (no #, **, -, ```, etc.). Use simple line breaks and dashes for structure.
