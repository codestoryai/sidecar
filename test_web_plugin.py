from plugins.web_plugin import WebPlugin

def main():
    query = "What happened in the news today?"
    plugin = WebPlugin()  # using default parameters
    results = plugin.run(query)
    print(results)

if __name__ == "__main__":
    main()