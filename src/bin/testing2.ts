import * as syncRequest from 'sync-request';

function run(
    request: OllamaEmbeddingTypes.Request,
    options: OllamaEmbeddingTypes.RequestOptions,
): OllamaEmbeddingTypes.Response {
    const url = options.apiUrl || OLLAMA_EMBEDDING_URL;

    const response = syncRequest('POST', url, {
        headers: options.headers || {},
        json: request,
    });

    return JSON.parse(response.getBody('utf8'));
}