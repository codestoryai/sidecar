import datetime

class WebPlugin:
    def __init__(self, max_results=5, search_prompt=None):
        self.max_results = max_results
        self.search_prompt = search_prompt or (
            f"A web search was conducted on {datetime.datetime.utcnow().isoformat()}Z. "
            "Incorporate the following web search results into your response. "
            "IMPORTANT: Cite them using markdown links named using the domain of the source. "
            "Example: [nytimes.com](https://nytimes.com/some-page)."
        )

    def search(self, query):
        # Simulate a web search call by returning dummy results.
        dummy_results = []
        for i in range(min(self.max_results, 1)):
            dummy_result = {
                "title": f"Dummy Article {i+1}",
                "url": f"https://example.com/dummy-article-{i+1}",
                "date": datetime.datetime.utcnow().isoformat() + "Z",
                "summary": "This is a dummy article summary with key points relevant to the query."
            }
            dummy_results.append(dummy_result)
        return dummy_results

    def format_results(self, results):
        formatted = self.search_prompt + "\n\n"
        for r in results:
            formatted += (
                f"Title: {r['title']}\n"
                f"URL: {r['url']}\n"
                f"Date: {r['date']}\n"
                f"Summary: {r['summary']}\n\n"
            )
        return formatted

    def run(self, query):
        results = self.search(query)
        return self.format_results(results)