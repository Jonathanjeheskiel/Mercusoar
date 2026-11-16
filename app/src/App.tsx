import { useState } from "react";
import SetupScreen from "./screens/SetupScreen";
import { SetupConfig } from "./types";
import "./App.css";

export default function App() {
  // null = still on the setup screen
  const [config, setConfig] = useState<SetupConfig | null>(null);

  if (!config) return <SetupScreen onStart={setConfig} />;

  // Temporary: the student and assistant screens are built in steps 2 and 3.
  return (
    <main className="setup">
      <div className="card setup-card">
        <h1>Setup complete</h1>
        <pre className="config-preview">{JSON.stringify(config, null, 2)}</pre>
        <p className="help">
          The {config.role === "student" ? "student" : "assistant"} screen comes in the next step.
        </p>
        <button className="btn-secondary" onClick={() => setConfig(null)}>Back to setup</button>
      </div>
    </main>
  );
}
