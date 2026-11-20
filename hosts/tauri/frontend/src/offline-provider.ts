import { createSemanticProvider } from "./bridge.js";

/** Convenience phrases belong to this app; the workflow comes from its catalog. */
export const offlineProvider = createSemanticProvider({
  aliases: { "保存": "fill_and_save", save: "fill_and_save" },
});
