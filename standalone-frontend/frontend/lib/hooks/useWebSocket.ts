import { useEffect, useRef, useCallback } from 'react';

type MessageHandler = (payload: any) => void;

interface WebSocketMessage {
  type: string;
  payload: any;
}

export function useWebSocket() {
  const ws = useRef<WebSocket | null>(null);
  const messageHandlers = useRef<Map<string, MessageHandler>>(new Map());

  useEffect(() => {
    if (!ws.current) {
      ws.current = new WebSocket('ws://localhost:3001/ws');
      
      ws.current.onopen = () => {
        console.log('WebSocket connected');
      };

      ws.current.onmessage = (event) => {
        try {
          const message: WebSocketMessage = JSON.parse(event.data);
          const handler = messageHandlers.current.get(message.type);
          if (handler) {
            handler(message.payload);
          }
        } catch (error) {
          console.error('Error processing WebSocket message:', error);
        }
      };

      ws.current.onerror = (error) => {
        console.error('WebSocket error:', error);
      };

      ws.current.onclose = () => {
        console.log('WebSocket disconnected');
        // Attempt to reconnect after a delay
        setTimeout(() => {
          ws.current = null;
        }, 5000);
      };
    }

    return () => {
      if (ws.current) {
        ws.current.close();
      }
    };
  }, []);

  const send = useCallback((message: WebSocketMessage) => {
    if (ws.current?.readyState === WebSocket.OPEN) {
      ws.current.send(JSON.stringify(message));
    } else {
      console.error('WebSocket is not connected');
    }
  }, []);

  const onMessage = useCallback((type: string, handler: MessageHandler) => {
    messageHandlers.current.set(type, handler);
  }, []);

  return {
    send,
    onMessage,
    isConnected: ws.current?.readyState === WebSocket.OPEN
  };
}