// Loads a QML JavaScript library (`.pragma library`, `.import "x.js" as X`)
// into Node so the models can be unit-tested without a Qt runtime.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const modelsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "models");
const IMPORT = /^\.import\s+"([^"]+\.js)"\s+as\s+(\w+)\s*$/;

export function loadModel(fileName) {
  const source = readFileSync(join(modelsDir, fileName), "utf8");
  const context = vm.createContext({});
  const body = source
    .split("\n")
    .map((line) => {
      const trimmed = line.trim();
      if (trimmed === ".pragma library") return "";
      const match = IMPORT.exec(trimmed);
      if (match) {
        context[match[2]] = loadModel(match[1]);
        return "";
      }
      return line;
    })
    .join("\n");
  vm.runInContext(body, context, { filename: fileName });
  return context;
}
