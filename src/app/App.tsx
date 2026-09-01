import { ChatPanel } from "../features/chat/ChatPanel";
import { PetSurface } from "../features/pet/PetSurface";
import { SettingsPanel } from "../features/settings/SettingsPanel";

interface AppProps {
  windowLabel: string;
}

export function App({ windowLabel }: AppProps) {
  switch (windowLabel) {
    case "settings":
      return <SettingsPanel />;
    case "chat":
      return <ChatPanel />;
    case "pet":
    default:
      return <PetSurface status="idle" />;
  }
}
