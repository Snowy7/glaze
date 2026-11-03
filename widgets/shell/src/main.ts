import "./style.css";
import { createEditor } from "./edit";
import { createFrames } from "./frames";
import { listen, send } from "./host";

const root = document.querySelector<HTMLElement>("#root");
const layer = document.querySelector<HTMLElement>("#layer");
if (!root || !layer) throw new Error("shell markup is missing");

const frames = createFrames({ layer, send, onCreate: (slot) => editor.decorate(slot) });
const editor = createEditor({ root, frames, send });

listen((message) => {
  if (message.type === "state") {
    editor.reset();
    frames.sync(message);
    editor.sync(message);
  } else {
    frames.onHost(message);
  }
});

send({ type: "ready" });
