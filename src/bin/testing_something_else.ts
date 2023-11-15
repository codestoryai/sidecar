import * as vscode from 'vscode';
import { v4 as uuidv4 } from 'uuid';
import logger from '../logger'; // Import the logger

// ...

export class CSChatProvider implements vscode.CSChatSessionProvider {
    private _logger: any; // Add a logger object to the class

    constructor(
        workingDirectory: string,
        codeGraph: CodeGraph,
        repoName: string,
        repoHash: string,
        codeSymbolsLanguageCollection: CodeSymbolsLanguageCollection,
        testSuiteRunCommand: string,
        activeFilesTracker: ActiveFilesTracker,
        uniqueUserId: string,
        agentCustomInstruction: string | null,
        sideCarClient: SideCarClient,
        repoRef: RepoRef,
        projectContext: ProjectContext,
    ) {
        this._workingDirectory = workingDirectory;
        this._codeGraph = codeGraph;
        this._chatSessionState = new CSChatSessionState(
            agentCustomInstruction,
        );
        this._repoHash = repoHash;
        this._repoName = repoName;
        this._codeSymbolsLanguageCollection = codeSymbolsLanguageCollection;
        this._testSuiteRunCommand = testSuiteRunCommand;
        this._activeFilesTracker = activeFilesTracker;
        this._uniqueUserId = uniqueUserId;
        this._agentCustomInformation = agentCustomInstruction;
        this._sideCarClient = sideCarClient;
        this._currentRepoRef = repoRef;
        this._projectContext = projectContext;
        this._logger = logger; // Initialize the logger
    }

    // ...

    provideSlashCommands?(session: CSChatSession, token: vscode.CancellationToken): vscode.ProviderResult<vscode.CSChatSessionSlashCommand[]> {
        this._logger.info('provideSlashCommands', session); // Log the inputs
        // ...
    }

    // ...

    prepareSession(initialState: CSChatSessionState | undefined, token: CSChatCancellationToken): vscode.ProviderResult<CSChatSession> {
        this._logger.info('prepareSession', initialState, token); // Log the inputs
        // ...
    }

    // ...

    resolveRequest(session: CSChatSession, context: CSChatRequestArgs | string, token: CSChatCancellationToken): vscode.ProviderResult<CSChatRequest> {
        this._logger.info('resolveRequest', session, context, token); // Log the inputs
        // ...
    }

    // ...

    provideResponseWithProgress(request: CSChatRequest, progress: vscode.Progress<CSChatProgress>, token: CSChatCancellationToken): vscode.ProviderResult<CSChatResponseForProgress> {
        this._logger.info('provideResponseWithProgress', request, progress, token); // Log the inputs
        // ...
    }

    // ...

    removeRequest(session: CSChatSession, requestId: string) {
        this._logger.info('removeRequest', session, requestId); // Log the inputs
        // ...
    }
}