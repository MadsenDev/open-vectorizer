import { ChangeEvent, useEffect, useMemo, useState } from 'react';
import clsx from 'clsx';

type WasmModule = typeof import('./pkg/png2svg_core.js');

const MAX_COLORS = 32;

type Mode = 'logo' | 'poster' | 'pixel';

interface UiOptions {
  colors: number;
  detail: number;
  smoothness: number;
  mode: Mode;
}

const presets: { label: string; options: UiOptions }[] = [
  {
    label: 'Logo (Clean)',
    options: { colors: 6, detail: 0.65, smoothness: 0.7, mode: 'logo' },
  },
  {
    label: 'Poster (More Detail)',
    options: { colors: 16, detail: 0.9, smoothness: 0.5, mode: 'poster' },
  },
  {
    label: 'Pixel Art (Crisp)',
    options: { colors: 12, detail: 0.4, smoothness: 0.3, mode: 'pixel' },
  },
];

const defaultOptions: UiOptions = presets[0].options;

function generatePlaceholderSvg(options: UiOptions, width = 420, height = 280) {
  const paletteCount = Math.max(2, Math.min(MAX_COLORS, Math.round(options.colors * options.detail)));
  const blockWidth = Math.max(12, Math.floor(width / paletteCount));
  const opacity = (0.35 + options.smoothness * 0.5).toFixed(2);
  const hueOffset = options.mode === 'poster' ? 24 : options.mode === 'pixel' ? 180 : 0;

  const rows = Math.ceil(height / blockWidth);
  const cols = Math.ceil(width / blockWidth);

  const blocks: string[] = [];
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const hue = (hueOffset + (x * 13 + y * 17) * options.detail * 30) % 360;
      const sat = 55 + (options.mode === 'pixel' ? 10 : 20) + Math.sin((x + y) / 3) * 10;
      const light = 35 + options.smoothness * 25 + Math.cos((x + 1) * (y + 2)) * 2;
      blocks.push(
        `<rect x="${x * blockWidth}" y="${y * blockWidth}" width="${blockWidth}" height="${blockWidth}" fill="hsl(${hue.toFixed(1)}, ${sat.toFixed(0)}%, ${light.toFixed(0)}%)" fill-opacity="${opacity}" />`,
      );
    }
  }

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" aria-label="Vector preview" shape-rendering="${
    options.mode === 'pixel' ? 'crispEdges' : 'geometricPrecision'
  }">${blocks.join('')}</svg>`;
}

function formatPercent(value: number) {
  return `${Math.round(value * 100)}%`;
}

function App() {
  const [options, setOptions] = useState<UiOptions>(defaultOptions);
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [pngPreviewUrl, setPngPreviewUrl] = useState<string | null>(null);
  const [svgMarkup, setSvgMarkup] = useState<string>('');
  const [wasmModule, setWasmModule] = useState<WasmModule | null>(null);
  const [wasmReady, setWasmReady] = useState(false);
  const [wasmError, setWasmError] = useState<string | null>(null);
  const [isVectorizing, setIsVectorizing] = useState(false);
  const [vectorizeError, setVectorizeError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function loadWasm() {
      try {
        const module: WasmModule = await import('./pkg/png2svg_core.js');
        await module.default();
        if (cancelled) return;

        const defaults = JSON.parse(module.default_options_json()) as UiOptions;
        setWasmModule(module);
        setOptions((prev) => ({ ...defaults, ...prev }));
        setWasmReady(true);
      } catch (error) {
        console.error('[open-vectorizer] failed to load wasm', error);
        if (cancelled) return;
        setWasmError(
          'Failed to load WASM build. Run `wasm-pack build png2svg/core --target web --out-dir ../../web-ui/src/pkg --release` from the png2svg/core directory, or move the generated files to web-ui/src/pkg.',
        );
      }
    }

    loadWasm();

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (selectedFile) return;
    setSvgMarkup(generatePlaceholderSvg(options));
  }, [options, selectedFile]);

  const status = useMemo(() => {
    if (wasmError) return wasmError;
    if (!wasmReady) return 'Loading WASM build…';
    if (isVectorizing) return 'Vectorizing PNG…';
    if (!selectedFile) return 'Upload a PNG to begin';
    return `Ready to vectorize ${selectedFile.name}`;
  }, [isVectorizing, selectedFile, wasmError, wasmReady]);

  function handleFileChange(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) {
      setSelectedFile(null);
      setPngPreviewUrl(null);
      return;
    }

    setSelectedFile(file);
    const url = URL.createObjectURL(file);
    setPngPreviewUrl((prev) => {
      if (prev) URL.revokeObjectURL(prev);
      return url;
    });
  }

  useEffect(() => {
    if (!wasmReady || !wasmModule || !selectedFile) return;

    const currentFile = selectedFile;
    const currentModule = wasmModule;
    let cancelled = false;
    setIsVectorizing(true);
    setVectorizeError(null);

    async function runVectorizer() {
      try {
        const buffer = await currentFile.arrayBuffer();
        const optionsJson = JSON.stringify(options);
        console.log('[open-vectorizer] vectorizing with options:', optionsJson);
        const svg = currentModule.png_to_svg_wasm(new Uint8Array(buffer), optionsJson);
        console.log('[open-vectorizer] generated SVG length:', svg.length);
        console.log('[open-vectorizer] SVG preview (first 500 chars):', svg.substring(0, 500));
        
        // Check for unique colors in the SVG
        const colorMatches = svg.matchAll(/fill="#([0-9a-f]{6})"/gi);
        const uniqueColors = new Set<string>();
        let colorCount = 0;
        for (const match of colorMatches) {
          uniqueColors.add(match[1].toLowerCase());
          colorCount++;
          if (colorCount > 1000) break; // Sample first 1000 to avoid performance issues
        }
        console.log('[open-vectorizer] unique colors found (sampled):', uniqueColors.size, 'colors:', Array.from(uniqueColors).slice(0, 10));
        
        // Check viewBox dimensions
        const viewBoxMatch = svg.match(/viewBox="0 0 (\d+) (\d+)"/);
        if (viewBoxMatch) {
          console.log('[open-vectorizer] SVG dimensions:', viewBoxMatch[1], 'x', viewBoxMatch[2]);
        }
        
        if (!cancelled) {
          setSvgMarkup(svg);
        }
      } catch (error) {
        console.error('[open-vectorizer] vectorization failed', error);
        if (!cancelled) {
          const message = error instanceof Error ? error.message : 'Unknown error';
          setVectorizeError(`Vectorization failed: ${message}`);
          setSvgMarkup(generatePlaceholderSvg(options));
        }
      } finally {
        if (!cancelled) {
          setIsVectorizing(false);
        }
      }
    }

    runVectorizer();

    return () => {
      cancelled = true;
    };
  }, [options, selectedFile, wasmModule, wasmReady]);

  function updateOption<K extends keyof UiOptions>(key: K, value: UiOptions[K]) {
    setOptions((prev) => ({ ...prev, [key]: value }));
  }

  function applyPreset(presetOptions: UiOptions) {
    setOptions(presetOptions);
  }

  function presetMatches(candidate: UiOptions) {
    return (
      candidate.colors === options.colors &&
      candidate.detail === options.detail &&
      candidate.smoothness === options.smoothness &&
      candidate.mode === options.mode
    );
  }

  return (
    <div className="min-h-screen bg-slate-950 text-slate-100">
      <header className="border-b border-slate-800 bg-slate-900/40 backdrop-blur">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-6 py-4">
          <div>
            <p className="text-xs uppercase tracking-[0.2em] text-emerald-300">Open Vectorizer</p>
            <h1 className="text-xl font-semibold text-white">PNG → SVG Playground</h1>
            <p className="text-sm text-slate-400">Live controls to mirror the CLI and upcoming WASM build.</p>
          </div>
          <div className="flex items-center gap-3 text-sm text-slate-300">
            <span className="inline-flex h-2 w-2 rounded-full bg-emerald-400" aria-hidden />
            <span>{status}</span>
          </div>
        </div>
      </header>

      <main className="mx-auto flex max-w-6xl flex-col gap-6 px-6 py-8 lg:flex-row">
        <section className="w-full space-y-6 lg:w-1/3">
          <div className="rounded-2xl border border-slate-800 bg-slate-900/40 p-5 shadow-lg shadow-emerald-900/10">
            <h2 className="mb-4 text-lg font-semibold text-white">Input</h2>
            <label
              htmlFor="file"
              className={clsx(
                'flex h-32 cursor-pointer items-center justify-center rounded-xl border-2 border-dashed border-slate-700 bg-slate-900/60 text-center transition hover:border-emerald-400 hover:bg-slate-900/80',
                selectedFile ? 'text-slate-50' : 'text-slate-400',
              )}
            >
              <div className="space-y-1">
                <p className="text-sm font-medium">
                  {selectedFile ? 'Change PNG file' : 'Drop a PNG or click to browse'}
                </p>
                <p className="text-xs text-slate-500">Up to 10 MB • Anti-aliased assets work best</p>
              </div>
              <input id="file" type="file" accept="image/png" className="sr-only" onChange={handleFileChange} />
            </label>
          </div>

          <div className="rounded-2xl border border-slate-800 bg-slate-900/40 p-5 shadow-lg shadow-emerald-900/10">
            <h2 className="mb-4 text-lg font-semibold text-white">Presets</h2>
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-1">
              {presets.map((preset) => (
                <button
                  key={preset.label}
                  type="button"
                  onClick={() => applyPreset(preset.options)}
                  className={clsx(
                    'w-full rounded-lg border px-3 py-2 text-left text-sm font-medium transition',
                    presetMatches(preset.options)
                      ? 'border-emerald-400 bg-emerald-500/10 text-white'
                      : 'border-slate-700 bg-slate-900/60 text-slate-200 hover:border-emerald-400/70 hover:bg-slate-900',
                  )}
                >
                  {preset.label}
                </button>
              ))}
            </div>
          </div>

          <div className="rounded-2xl border border-slate-800 bg-slate-900/40 p-5 shadow-lg shadow-emerald-900/10 space-y-4">
            <h2 className="text-lg font-semibold text-white">Controls</h2>

            <div>
              <div className="flex items-center justify-between text-sm text-slate-300">
                <label htmlFor="colors" className="font-medium text-white">
                  Colors
                </label>
                <span className="text-slate-400">{options.colors} / {MAX_COLORS}</span>
              </div>
              <input
                id="colors"
                type="range"
                min={2}
                max={MAX_COLORS}
                value={options.colors}
                onChange={(e) => updateOption('colors', Number(e.target.value))}
                className="mt-2 h-2 w-full cursor-pointer appearance-none rounded-full bg-slate-800 accent-emerald-400"
              />
            </div>

            <div>
              <div className="flex items-center justify-between text-sm text-slate-300">
                <label htmlFor="detail" className="font-medium text-white">
                  Detail
                </label>
                <span className="text-slate-400">{formatPercent(options.detail)}</span>
              </div>
              <input
                id="detail"
                type="range"
                min={0.1}
                max={1}
                step={0.01}
                value={options.detail}
                onChange={(e) => updateOption('detail', Number(e.target.value))}
                className="mt-2 h-2 w-full cursor-pointer appearance-none rounded-full bg-slate-800 accent-emerald-400"
              />
            </div>

            <div>
              <div className="flex items-center justify-between text-sm text-slate-300">
                <label htmlFor="smoothness" className="font-medium text-white">
                  Smoothness
                </label>
                <span className="text-slate-400">{formatPercent(options.smoothness)}</span>
              </div>
              <input
                id="smoothness"
                type="range"
                min={0.1}
                max={1}
                step={0.01}
                value={options.smoothness}
                onChange={(e) => updateOption('smoothness', Number(e.target.value))}
                className="mt-2 h-2 w-full cursor-pointer appearance-none rounded-full bg-slate-800 accent-emerald-400"
              />
            </div>

            <div className="space-y-2">
              <p className="text-sm font-medium text-white">Mode</p>
              <div className="grid grid-cols-3 gap-2 text-sm">
                {(['logo', 'poster', 'pixel'] as Mode[]).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => updateOption('mode', mode)}
                    className={clsx(
                      'rounded-lg border px-3 py-2 capitalize transition',
                      options.mode === mode
                        ? 'border-emerald-400 bg-emerald-500/10 text-white'
                        : 'border-slate-700 bg-slate-900/60 text-slate-200 hover:border-emerald-400/70 hover:bg-slate-900',
                    )}
                  >
                    {mode === 'pixel' ? 'Pixel Art' : mode}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </section>

        <section className="w-full space-y-4 lg:w-2/3">
          <div className="flex flex-col gap-3 rounded-2xl border border-slate-800 bg-slate-900/40 p-5 shadow-lg shadow-emerald-900/10">
            <div className="flex items-center justify-between gap-2">
              <div>
                <h2 className="text-lg font-semibold text-white">Preview</h2>
                <p className="text-sm text-slate-400">Original PNG on the left, vector preview on the right.</p>
              </div>
              <div className="flex items-center gap-2 text-xs text-slate-400">
                <span
                  className={clsx(
                    'rounded-full px-2 py-1 font-semibold',
                    wasmReady ? 'bg-emerald-500/20 text-emerald-200' : 'bg-amber-500/20 text-amber-100',
                  )}
                >
                  {wasmReady ? 'WASM ready' : 'WASM build missing'}
                </span>
                <span
                  className={clsx(
                    'rounded-full px-2 py-1 font-semibold',
                    isVectorizing ? 'bg-emerald-500/20 text-emerald-200' : 'bg-slate-800 text-slate-200',
                  )}
                >
                  {isVectorizing ? 'Vectorizing…' : 'Idle'}
                </span>
              </div>
            </div>

            <div className="grid gap-4 lg:grid-cols-2">
              <div className="overflow-hidden rounded-xl border border-slate-800 bg-slate-950/60">
                <div className="border-b border-slate-800 px-4 py-2 text-xs uppercase tracking-[0.15em] text-slate-400">
                  PNG Input
                </div>
                <div className="flex h-72 items-center justify-center bg-slate-950">
                  {pngPreviewUrl ? (
                    <img src={pngPreviewUrl} alt="Uploaded PNG preview" className="max-h-full max-w-full object-contain" />
                  ) : (
                    <p className="text-sm text-slate-500">No file selected yet.</p>
                  )}
                </div>
              </div>

              <div className="overflow-hidden rounded-xl border border-slate-800 bg-slate-950/60">
                <div className="border-b border-slate-800 px-4 py-2 text-xs uppercase tracking-[0.15em] text-slate-400">
                  SVG Preview {selectedFile && wasmReady ? '(rendered)' : '(placeholder)'}
                </div>
                <div className="flex h-72 items-center justify-center bg-slate-950 overflow-auto p-4">
                  {svgMarkup ? (
                    <div
                      className="h-full w-full flex items-center justify-center [&>svg]:max-h-full [&>svg]:max-w-full [&>svg]:w-auto [&>svg]:h-auto [&>svg]:block"
                      role="img"
                      aria-label="Vectorized preview"
                      dangerouslySetInnerHTML={{ __html: svgMarkup }}
                      style={{ minHeight: '100%', minWidth: '100%' }}
                    />
                  ) : (
                    <p className="text-sm text-slate-500">Adjust settings to generate a preview.</p>
                  )}
                </div>
              </div>
            </div>
            {vectorizeError && (
              <div className="rounded-xl border border-amber-600/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-100">
                {vectorizeError}
              </div>
            )}
            {!wasmReady && wasmError && (
              <div className="rounded-xl border border-amber-600/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-100">
                {wasmError}
              </div>
            )}
          </div>

          <div className="rounded-2xl border border-emerald-900/60 bg-emerald-500/5 p-4 text-sm text-emerald-100">
            <p className="font-semibold text-emerald-200">WASM pipeline ready</p>
            <p className="text-emerald-100/90">
              Build the WebAssembly bundle with
              <code className="mx-1 rounded bg-emerald-500/10 px-1 py-0.5 font-mono text-xs text-emerald-100">wasm-pack build --target web --out-dir ../../web-ui/src/pkg --release</code>
              from the <code className="mx-1 rounded bg-emerald-500/10 px-1 py-0.5 font-mono text-xs text-emerald-100">png2svg/core</code> directory.
            </p>
          </div>
        </section>
      </main>
    </div>
  );
}

export default App;
