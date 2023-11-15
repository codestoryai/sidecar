/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/
import * as vscode from 'vscode';
import { v4 as uuidv4 } from 'uuid';

import logger from '../logger';
import { CSChatState } from '../chatState/state';
import { getSelectedCodeContext, getSelectedCodeContextForExplain } from '../utilities/getSelectionContext';
import { generateChatCompletion, generateChatCompletionAx } from '../chatState/openai';
import { logChatPrompt, logSearchPrompt } from '../posthog/logChatPrompt';
import { formatPathsInAnswer, reportFromStreamToProgress, reportFromStreamToSearchProgress } from '../chatState/convertStreamToMessage';
import { CodeGraph } from '../codeGraph/graph';
import { createContextPrompt, getContextForPromptFromUserContext, getRelevantContextForCodeSelection } from '../chatState/getContextForCodeSelection';
import { debuggingFlow } from '../llm/recipe/debugging';
import { ToolingEventCollection } from '../timeline/events/collection';
import { ActiveFilesTracker } from '../activeChanges/activeFilesTracker';
import { deterministicClassifier, promptClassifier } from '../chatState/promptClassifier';
import { CodeSymbolsLanguageCollection } from '../languages/codeSymbolsLanguageCollection';
import { RepoRef, SideCarClient } from '../sidecar/client';
import { getLSPGraphContextForChat } from '../editor/activeView/ranges';
import { DeepContextForView } from '../sidecar/types';
import { ProjectContext } from '../utilities/workspaceContext';

class CSChatSessionState implements vscode.CSChatSessionState {
    public chatContext: CSChatState;

    constructor(agentCustomInstruction: string | null) {
        this.chatContext = new CSChatState(
            agentCustomInstruction,
        );
    }
}

class CSChatParticipant implements vscode.CSChatSessionParticipantInformation {
    name: string;
    icon?: vscode.Uri | undefined;

    constructor(name: string, icon?: vscode.Uri | undefined) {
        this.name = name;
        this.icon = icon;
    }

    toString(): string {
        return `CSChatParticipant { name: "${this.name}", icon: "${this.icon?.toString()}" }`;
    }
}

class CSChatSession implements vscode.CSChatSession {
    requester: CSChatParticipant;
    responder: CSChatParticipant;
    inputPlaceholder?: string | undefined;
    agentCustomInstruction: string | null;
    threadId: string;
    public chatSessionState: CSChatSessionState;

    saveState(): CSChatSessionState {
        logger.info('Saving state' + this.toString());
        return this.chatSessionState;
    }

    constructor(
        requester: CSChatParticipant,
        responder: CSChatParticipant,
        initialState: CSChatSessionState | undefined,
        agentCustomInstruction: string | null,
        inputPlaceholder?: string | undefined,
    ) {
        this.threadId = uuidv4();
        this.requester = requester;
        this.responder = responder;
        this.inputPlaceholder = inputPlaceholder;
        this.agentCustomInstruction = agentCustomInstruction;
        this.chatSessionState = initialState ?? new CSChatSessionState(
            this.agentCustomInstruction,
        );
    }

    toString(): string {
        return `CSChatSession { requester: ${this.requester.toString()}, responder: ${this.responder.toString()}, inputPlaceholder: "${this.inputPlaceholder}" }`;
    }
}

class CSChatRequestArgs implements vscode.CSChatSessionRequestArgs {
    command: string;
    args: any;

    constructor(command: string, args: any) {
        this.command = command;
        this.args = args;
    }

    toString(): string {
        return `CSChatRequestArgs { command: "${this.command}", args: ${JSON.stringify(this.args, null, 2)} }`;
    }
}

class CSChatReplyFollowup implements vscode.CSChatSessionReplyFollowup {
    message: string;
    tooltip?: string | undefined;
    title?: string | undefined;
    metadata?: any;

    constructor(message: string, tooltip?: string | undefined, title?: string | undefined, metadata?: any) {
        this.message = message;
        this.tooltip = tooltip;
        this.title = title;
        this.metadata = metadata;
    }

    toString(): string {
        return `CSChatReplyFollowup { message: "${this.message}", tooltip: "${this.tooltip}", title: "${this.title}", metadata: ${JSON.stringify(this.metadata, null, 2)} }`;
    }
}

export class CSChatRequest implements vscode.CSChatRequest {
    session: CSChatSession;
    message: string;
    variables: Record<string, vscode.CSChatVariableValue[]>;
    userProvidedContext: vscode.InteractiveUserProvidedContext | undefined;

    constructor(session: CSChatSession, message: string, variables: Record<string, vscode.CSChatVariableValue[]> = {}) {
        this.session = session;
        this.message = message;
        this.variables = variables;
    }

    toString(): string {
        return `CSChatRequest { session: ${this.session.toString()}, message: ${this.message.toString()}, variables: ${JSON.stringify(this.variables, null, 2)} }`;
    }
}

class CSChatResponseErrorDetails implements vscode.CSChatResponseErrorDetails {
    message: string;
    responseIsIncomplete?: boolean | undefined;
    responseIsFiltered?: boolean | undefined;

    constructor(message: string, responseIsIncomplete?: boolean | undefined, responseIsFiltered?: boolean | undefined) {
        this.message = message;
        this.responseIsIncomplete = responseIsIncomplete;
        this.responseIsFiltered = responseIsFiltered;
    }

    toString(): string {
        return `CSChatResponseErrorDetails { message: "${this.message}", responseIsIncomplete: "${this.responseIsIncomplete}", responseIsFiltered: "${this.responseIsFiltered}" }`;
    }
}

export class CSChatProgressContent implements vscode.CSChatProgressContent {
    content: string | vscode.MarkdownString;

    constructor(content: string | vscode.MarkdownString) {
        this.content = content;
    }

    toString(): string {
        return `CSChatProgressContent { content: "${this.content}" }`;
    }
}

export class CSChatProgressUsedContext implements vscode.CSChatProgressUsedContext {
    documents: vscode.DocumentContext[];

    constructor(documents: vscode.DocumentContext[]) {
        this.documents = documents;
    }

    toString(): string {
        return `CSChatProgressUsedContext { documents: ${JSON.stringify(this.documents, null, 2)} }`;
    }
}

export class CSChatContentReference implements vscode.CSChatContentReference {
    reference: vscode.Uri | vscode.Location;

    constructor(reference: vscode.Uri | vscode.Location) {
        this.reference = reference;
    }

    toString(): string {
        return `CSChatContentReference { reference: "${this.reference}" }`;
    }
}

export class CSChatInlineContentReference implements vscode.CSChatInlineContentReference {
    inlineReference: vscode.Uri | vscode.Location;
    title?: string;

    constructor(inlineReference: vscode.Uri | vscode.Location, title?: string) {
        this.inlineReference = inlineReference;
    }

    toString(): string {
        return `CSChatInlineContentReference { inlineReference: "${this.inlineReference}", title: "${this.title}" }`;
    }
}

class CSChatProgressId implements vscode.CSChatProgressId {
    responseId: string;

    constructor(responseId: string) {
        this.responseId = responseId;
    }

    toString(): string {
        return `CSChatProgressId { responseId: "${this.responseId}" }`;
    }
}

export class CSChatFileTreeData implements vscode.FileTreeData {
    label: string;
    uri: vscode.Uri;
    children?: vscode.FileTreeData[] | undefined;

    constructor(label: string, uri: vscode.Uri, children?: vscode.FileTreeData[] | undefined) {
        this.label = label;
        this.uri = uri;
        this.children = children;
    }

    toString(): string {
        return `CSChatFileTreeData { label: "${this.label}", uri: "${this.uri}", children: ${JSON.stringify(this.children, null, 2)} }`;
    }
}

export class CSChatProgressFileTree implements vscode.CSChatProgressFileTree {
    treeData: CSChatFileTreeData;

    constructor(treeData: CSChatFileTreeData) {
        this.treeData = treeData;
    }

    toString(): string {
        return `CSChatProgressFileTree { treeData: "${this.treeData}" }`;
    }
}

export class CSChatProgressTask implements vscode.CSChatProgressTask {
    placeholder: string;
    resolvedContent: Thenable<CSChatProgressContent | CSChatProgressFileTree>;

    constructor(placeholder: string, resolvedContent: Thenable<CSChatProgressContent | CSChatProgressFileTree>) {
        this.placeholder = placeholder;
        this.resolvedContent = resolvedContent;
    }

    toString(): string {
        return `CSChatProgressTask { placeholder: "${this.placeholder}", resolvedContent: "${this.resolvedContent}" }`;
    }
}

export type CSChatProgress = CSChatProgressContent | CSChatProgressId | CSChatProgressTask | CSChatProgressFileTree | CSChatProgressUsedContext | CSChatContentReference | CSChatInlineContentReference;

class CSChatResponseForProgress implements vscode.CSChatResponseForProgress {
    errorDetails?: CSChatResponseErrorDetails | undefined;

    constructor(errorDetails?: CSChatResponseErrorDetails | undefined) {
        this.errorDetails = errorDetails;
    }

    toString(): string {
        return `CSChatResponseForProgress { errorDetails: ${this.errorDetails?.toString()} }`;
    }
}

export class CSChatCancellationToken implements vscode.CancellationToken {
    isCancellationRequested: boolean;
    onCancellationRequested: vscode.Event<any>;

    constructor(isCancellationRequested: boolean, onCancellationRequested: vscode.Event<any>) {
        this.isCancellationRequested = isCancellationRequested;
        this.onCancellationRequested = onCancellationRequested;
    }

    toString(): string {
        return `CSChatCancellationToken { isCancellationRequested: "${this.isCancellationRequested}", onCancellationRequested: "${this.onCancellationRequested}" }`;
    }
}

export class CSChatProvider implements vscode.CSChatSessionProvider {
    private _chatSessionState: CSChatSessionState;

    private _codeGraph: CodeGraph;
    private _codeSymbolsLanguageCollection: CodeSymbolsLanguageCollection;
    private _workingDirectory: string;
    private _testSuiteRunCommand: string;
    private _activeFilesTracker: ActiveFilesTracker;
    private _repoName: string;
    private _repoHash: string;
    private _uniqueUserId: string;
    private _agentCustomInformation: string | null;
    private _sideCarClient: SideCarClient;
    private _currentRepoRef: RepoRef;
    private _projectContext: ProjectContext;

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
    }

    provideSlashCommands?(session: CSChatSession, token: vscode.CancellationToken): vscode.ProviderResult<vscode.CSChatSessionSlashCommand[]> {
        logger.info('provideSlashCommands', session);
        return [
            {
                command: 'help',
                kind: vscode.CompletionItemKind.Text,
                detail: 'Get help on how to use CodeStory',
                shouldRepopulate: true,
                followupPlaceholder: 'Ask me a question or type \'/\' for bb?',
                executeImmediately: false,
            },
            {
                command: 'agent',
                kind: vscode.CompletionItemKind.Text,
                detail: 'Invoke the CodeStory agent to do codebase wide changes',
                shouldRepopulate: true,
                executeImmediately: false,
            },
            {
                command: 'explain',
                kind: vscode.CompletionItemKind.Text,
                detail: 'Explain the code for the selection at a local and global level',
                shouldRepopulate: true,
                executeImmediately: false,
            },
            {
                command: 'general',
                kind: vscode.CompletionItemKind.Text,
                detail: 'Ask any kind of general questions to the AI with our without context',
                shouldRepopulate: true,
                executeImmediately: false,
            },
            {
                command: 'search',
                kind: vscode.CompletionItemKind.Text,
                detail: 'Search for the relevant code symbols from the codebase',
                shouldRepopulate: true,
                executeImmediately: false,
            },
        ];
    }

    provideWelcomeMessage?(token: CSChatCancellationToken): vscode.ProviderResult<vscode.CSChatWelcomeMessageContent[]> {
        logger.info('provideWelcomeMessage', token);
        return [
            'Hi, I\'m **Aide**, your personal AI assistant! I can write, debug, find, understand and explain code for you.',
            'Here are some things you can ask me to get started:',
            [
                new CSChatReplyFollowup(
                    '/agent add comments to the entrypoint',
                    'Add comments to the entrypoint',
                    'Add comments to the entrypoint'
                ),
                new CSChatReplyFollowup(
                    '/explain the active file in the editor',
                    'Explain the active file in the editor',
                    'Explain the active file in the editor'
                ),
                new CSChatReplyFollowup(
                    'Where are the race conditions in the selected code?',
                ),
            ],
            'You can start your request with \'**`/`**\' to give me specific instructions and type \'**`@`**\' to find files and code symbols that I can use to narrow down the context of your request.',
            'I might make mistakes, though! Like we all do once in a while. If you find me doing so, please share your feedback via **[Discord](https://discord.gg/Cwg3vqgb)** or **founders@codestory.ai** so I can get better.',
        ];
    }

    prepareSession(initialState: CSChatSessionState | undefined, token: CSChatCancellationToken): vscode.ProviderResult<CSChatSession> {
        logger.info('prepareSession', initialState, token);
        const iconUri = vscode.Uri.joinPath(
            vscode.extensions.getExtension('codestory-ghost.codestoryai')?.extensionUri ?? vscode.Uri.parse(''),
            'assets',
            'aide-white.svg'
        );
        return new CSChatSession(
            new CSChatParticipant('You'),
            new CSChatParticipant('Aide', iconUri),
            initialState,
            this._agentCustomInformation,
            'Ask away and use # to refer code symbols and files',
        );
    }

    resolveRequest(session: CSChatSession, context: CSChatRequestArgs | string, token: CSChatCancellationToken): vscode.ProviderResult<CSChatRequest> {
        logger.info('resolveRequest', session, context, token);
        // Here there can be actions from the / commands or just normal string
        // followup, so we need to handle both of them separately
        session.chatSessionState.chatContext.addUserMessage(context.toString());
        logger.info(`[codestory][message_length][resolveRequest] ${this._chatSessionState.chatContext.getMessageLength()}`);
        return new CSChatRequest(session, context.toString());
    }

    provideResponseWithProgress(request: CSChatRequest, progress: vscode.Progress<CSChatProgress>, token: CSChatCancellationToken): vscode.ProviderResult<CSChatResponseForProgress> {
        return (async () => {
            // export type UserMessageType = 'explain' | 'general' | 'instruction' | 'search' | 'help';
            const deterministicRequestType = deterministicClassifier(request.message.toString());
            const requestType = deterministicRequestType;
            const userProvidedContext = request.userProvidedContext ? getContextForPromptFromUserContext(request.userProvidedContext) : null;
            logger.info(`[codestory][request_type][provideResponseWithProgress] ${requestType}`);
            if (requestType === 'help') {
                progress.report(new CSChatProgressContent(
                    `Here are some helpful docs for resolving the most common issues: [Code Story](https://docs.codestory.ai)\n`
                ));
                return new CSChatResponseForProgress();
            } else if (requestType === 'instruction') {
                const prompt = request.message.toString().slice(7).trim();
                if (prompt.length === 0) {
                    return new CSChatResponseForProgress(new CSChatResponseErrorDetails('Please provide a prompt for the agent to work on'));
                }

                const toolingEventCollection = new ToolingEventCollection(
                    `/tmp/${uuidv4()}`,
                    { progress, cancellationToken: token },
                    prompt,
                );

                const uniqueId = uuidv4();
                await debuggingFlow(
                    prompt,
                    toolingEventCollection,
                    this._sideCarClient,
                    this._codeSymbolsLanguageCollection,
                    this._workingDirectory,
                    this._testSuiteRunCommand,
                    this._activeFilesTracker,
                    request.userProvidedContext,
                    uniqueId,
                    this._agentCustomInformation,
                    this._currentRepoRef,
                );
                return new CSChatResponseForProgress();
            } else if (requestType === 'explain') {
                // Implement the explain feature here
                const explainString = request.message.toString().slice('/explain'.length).trim();
                const currentSelection = getSelectedCodeContextForExplain(this._workingDirectory, this._currentRepoRef);
                if (currentSelection === null) {
                    progress.report(new CSChatProgressContent('Selecting code on the editor can help us explain it better'));
                    return new CSChatResponseForProgress();
                } else {
                    const explainResponse = await this._sideCarClient.explainQuery(explainString, this._currentRepoRef, currentSelection, request.session.threadId);
                    await reportFromStreamToSearchProgress(explainResponse, progress, token, this._currentRepoRef, this._workingDirectory);
                    return new CSChatResponseForProgress();
                }
            } else if (requestType === 'search') {
                logSearchPrompt(
                    request.message.toString(),
                    this._repoName,
                    this._repoHash,
                    this._uniqueUserId,
                );
                const searchString = request.message.toString().slice('/search'.length).trim();
                const searchResponse = await this._sideCarClient.searchQuery(searchString, this._currentRepoRef, request.session.threadId);
                await reportFromStreamToSearchProgress(searchResponse, progress, token, this._currentRepoRef, this._workingDirectory);
                // We get back here a bunch of responses which we have to pass properly to the agent
                return new CSChatResponseForProgress();
            } else {
                this._chatSessionState.chatContext.cleanupChatHistory();
                this._chatSessionState.chatContext.addUserMessage(request.message.toString());
                const query = request.message.toString().trim();
                logChatPrompt(
                    request.message.toString(),
                    this._repoName,
                    this._repoHash,
                    this._uniqueUserId,
                );
                const followupResponse = await this._sideCarClient.followupQuestion(query, this._currentRepoRef, request.session.threadId, request.variables);
                await reportFromStreamToSearchProgress(followupResponse, progress, token, this._currentRepoRef, this._workingDirectory);
                return new CSChatResponseForProgress();
            }
        })();
    }

    removeRequest(session: CSChatSession, requestId: string) {
        logger.info('removeRequest', session, requestId);
        // Do nothing
    }
}