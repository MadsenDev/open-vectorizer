/// <reference lib="webworker" />

// Vectorization runs here rather than on the main thread. A 1024px logo takes a
// few hundred milliseconds in wasm and a 2048px one takes seconds; on the main
// thread that reads as a frozen tab.

import init, { vectorize_rgba_report_wasm } from './pkg/png2svg_core.js';
import wasmUrl from './pkg/png2svg_core_bg.wasm?url';

export interface VectorizeRequest {
  id: number;
  width: number;
  height: number;
  /** Non-premultiplied RGBA8, transferred rather than copied. */
  rgba: ArrayBuffer;
  optionsJson: string;
}

export interface VectorizeReport {
  svg: string;
  shapes: number;
  nodes: number;
  circles: number;
  ellipses: number;
  rects: number;
  accuracy: number;
}

export type VectorizeResponse =
  | { id: number; ok: true; report: VectorizeReport; elapsedMs: number }
  | { id: number; ok: false; error: string };

let ready: Promise<unknown> | null = null;

function startup(): Promise<unknown> {
  // Vite rewrites the `?url` import to a hashed asset path, which is more
  // reliable than letting the generated glue guess its own location.
  ready ??= init({ module_or_path: wasmUrl });
  return ready;
}

self.onmessage = async (event: MessageEvent<VectorizeRequest>) => {
  const { id, width, height, rgba, optionsJson } = event.data;

  try {
    await startup();
    const started = performance.now();
    const json = vectorize_rgba_report_wasm(
      width,
      height,
      new Uint8Array(rgba),
      optionsJson,
    );
    const response: VectorizeResponse = {
      id,
      ok: true,
      report: JSON.parse(json) as VectorizeReport,
      elapsedMs: performance.now() - started,
    };
    self.postMessage(response);
  } catch (error) {
    const response: VectorizeResponse = {
      id,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    };
    self.postMessage(response);
  }
};
