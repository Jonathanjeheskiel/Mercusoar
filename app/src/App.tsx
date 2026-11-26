import { useState } from "react";
import AssistantScreen from "./screens/AssistantScreen";
import SetupScreen from "./screens/SetupScreen";
import StudentScreen from "./screens/StudentScreen";
import { SetupConfig } from "./types";
import "./App.css";
import "./screens.css";

export default function App() {
  // null = still on the setup screen
  const [config, setConfig] = useState<SetupConfig | null>(null);
  const exit = () => setConfig(null);

  if (!config) return <SetupScreen onStart={setConfig} />;

  if (config.role === "student") {
    return <StudentScreen pc={config.pc} hubAddress={config.hubAddress} onExit={exit} />;
  }

  return (
    <AssistantScreen
      seat={config.seat}
      hostHub={config.hostHub}
      hubAddress={config.hubAddress}
      passcode={config.passcode}
      onExit={exit}
    />
  );
}
