// wasm-engine.js — one engine instance, shared by every live demo on a page.
//
// wasm-bindgen's init() builds a fresh WebAssembly.Memory each time it runs.
// Two demos each calling it would leave one of them holding trace handles
// into an instance the other had already replaced — which surfaces as a
// detached RGBA view ("Invalid typed array length") and as errors from a plot
// that belongs to the other world. So the module is initialised exactly once
// and every demo awaits the same promise.

let booting = null;

export function engine() {
  if (!booting) {
    booting = (async () => {
      // ?v=1 flushes copies cached from before the site sent Cache-Control
      // headers; the explicit wasm URL keeps the pair on the same version.
      const mod = await import('../pkg/plotui_wasm.js?v=1');
      const wasm = await mod.default({
        module_or_path: new URL('../pkg/plotui_wasm_bg.wasm?v=1', import.meta.url),
      });
      // `mod` is handed back whole so a demo needing more of the API
      // (ForceLayout, marching_cubes, the sweep helpers) can reach it without
      // a second init.
      return { Plot: mod.Plot, memory: wasm.memory, mod };
    })();
  }
  return booting;
}
