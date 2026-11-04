import "./style.css";
import { createEditor, type Editor } from "./edit";
import { createFrames } from "./frames";
import { listen, send } from "./host";
import { layerOf } from "./placement";

const root = document.querySelector<HTMLElement>("#root");
const layer = document.querySelector<HTMLElement>("#layer");
if (!root || !layer) throw new Error("shell markup is missing");

// The overlay layer only shows the widgets it is given: no grid, no edit chrome, so no editor.
let editor: Editor | undefined;
const frames = createFrames({ layer, send, onCreate: (slot) => editor?.decorate(slot) });

listen((message) => {
  if (message.type === "state") {
    if (layerOf(message) === "desktop") {
      editor ??= createEditor({ root, frames, send });
      editor.reset();
    }
    frames.sync(message);
    editor?.sync(message);
  } else {
    frames.onHost(message);
  }
});

send({ type: "ready" });
