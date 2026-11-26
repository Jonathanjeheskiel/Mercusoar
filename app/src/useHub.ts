import { useEffect, useRef, useState } from "react";
import { ClientMessage, HUB_PORT, HubMessage } from "./protocol";

export type ConnectionStatus = "connecting" | "connected" | "disconnected" | "rejected";

/**
 * Keeps one WebSocket connection to the hub open.
 * - Sends `hello` every time it (re)connects, so the hub knows who we are.
 * - Calls `onMessage` for every message the hub sends.
 * - Reconnects every 2 seconds if the connection drops (e.g. hub restarted),
 *   except after a wrong passcode.
 *
 * Pass `address = null` to wait (e.g. while the hub is still starting).
 */
export function useHub(
  address: string | null,
  hello: ClientMessage | null,
  onMessage: (msg: HubMessage) => void,
) {
  const [status, setStatus] = useState<ConnectionStatus>("connecting");
  const socketRef = useRef<WebSocket | null>(null);

  // Keep the latest callback without reconnecting when it changes.
  const onMessageRef = useRef(onMessage);
  onMessageRef.current = onMessage;

  const helloText = hello ? JSON.stringify(hello) : null;

  useEffect(() => {
    if (!address || !helloText) return;
    let stopped = false;
    let retryTimer: number | undefined;

    function connect() {
      setStatus("connecting");
      const ws = new WebSocket(`ws://${address}:${HUB_PORT}`);
      socketRef.current = ws;

      ws.onopen = () => {
        ws.send(helloText!);
        setStatus("connected");
      };
      ws.onmessage = (event) => {
        const msg = JSON.parse(event.data) as HubMessage;
        if (msg.type === "auth_error") {
          stopped = true; // retrying with the same wrong passcode is pointless
          setStatus("rejected");
        }
        onMessageRef.current(msg);
      };
      ws.onclose = () => {
        if (stopped) return;
        setStatus("disconnected");
        retryTimer = window.setTimeout(connect, 2000);
      };
    }

    connect();
    return () => {
      stopped = true;
      window.clearTimeout(retryTimer);
      socketRef.current?.close();
    };
  }, [address, helloText]);

  function send(msg: ClientMessage) {
    const ws = socketRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
  }

  return { status, send };
}
