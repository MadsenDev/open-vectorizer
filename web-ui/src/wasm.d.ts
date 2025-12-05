declare module '/pkg/png2svg_core.js' {
  export function png_to_svg_wasm(png_bytes: Uint8Array, options_json: string): string;
  export function default_options_json(): string;
  export default function init(module?: WebAssembly.Module | RequestInfo | URL): Promise<unknown>;
}
