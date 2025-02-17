from plugins.web_plugin import WebPlugin

def integrate_web_search(conversation, web_plugin):
    """
    Given a conversation (list of messages) and a WebPlugin instance, this function:
    - Finds the most recent user message.
    - Uses its content as the query for the web plugin.
    - Inserts a new system message (with the plugin's output) immediately before that user message.
    """
    last_user_index = None
    for i in range(len(conversation) - 1, -1, -1):
        if conversation[i].get("role") == "user":
            last_user_index = i
            break

    # If no user message is found, return conversation unchanged.
    if last_user_index is None:
        return conversation

    # Use the content of the last user message as the query.
    query = conversation[last_user_index].get("content", "")
    # Execute the web search using the plugin.
    web_response = web_plugin.run(query)
    # Insert the system message with the search results above the last user message.
    conversation.insert(last_user_index, {"role": "system", "content": web_response})
    return conversation

if __name__ == "__main__":
    # Sample conversation for testing.
    conversation = [
        {"role": "system", "content": "System initialization."},
        {"role": "user", "content": "Latest tech news."}
    ]
    plugin = WebPlugin(max_results=1)
    updated_conversation = integrate_web_search(conversation, plugin)
    print("Updated Conversation:")
    for msg in updated_conversation:
        print(f"{msg['role']}: {msg['content']}\n")