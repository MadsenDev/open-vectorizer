import React, {
  ChangeEvent,
  DragEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import clsx from 'clsx';

import type {
  VectorizeReport,
  VectorizeRequest,
  VectorizeResponse,
} from './vectorizer.worker';

/**
 * Longest edge we hand to the engine, in pixels.
 *
 * Coverage is held as one full-canvas float field per palette colour, so memory
 * grows as `width * height * colours`. At 2048px with eight colours that is about
 * 150MB, which wasm32 handles; 4096px would be four times that and would fail on
 * mobile. Larger input is downscaled, and the page says so.
 */
const MAX_DIMENSION = 2048;

type Mode = 'auto' | 'logo' | 'poster' | 'pixel';

interface Options {
  colors: number;
  detail: number;
  smoothness: number;
  tolerance: number;
  mode: Mode;
}

const defaults: Options = {
  colors: 8,
  detail: 0.6,
  smoothness: 0.5,
  tolerance: 1.5,
  mode: 'auto',
};

const presets: { label: string; hint: string; options: Options }[] = [
  {
    label: 'Automatic',
    hint: 'Inspect the image and choose',
    options: defaults,
  },
  {
    label: 'Logo',
    hint: 'Few colours, clean curves',
    options: {
      colors: 6,
      detail: 0.65,
      smoothness: 0.72,
      tolerance: 1.4,
      mode: 'logo',
    },
  },
  {
    label: 'Poster',
    hint: 'More colours, more detail',
    options: {
      colors: 16,
      detail: 0.85,
      smoothness: 0.45,
      tolerance: 1.5,
      mode: 'poster',
    },
  },
  {
    label: 'Pixel art',
    hint: 'Exact cell edges, no smoothing',
    options: {
      colors: 12,
      detail: 1,
      smoothness: 0,
      tolerance: 0.5,
      mode: 'pixel',
    },
  },
];

interface Decoded {
  width: number;
  height: number;
  rgba: ArrayBuffer;
  originalWidth: number;
  originalHeight: number;
}

/**
 * Decode with the browser's own decoders and downscale if needed.
 *
 * Doing this here rather than in Rust means every format the browser reads is
 * supported — including WebP, AVIF and HEIC where available — and no image codec
 * is compiled into the wasm at all.
 */
async function decode(file: File): Promise<Decoded> {
  const bitmap = await createImageBitmap(file);
  const longest = Math.max(bitmap.width, bitmap.height);
  const scale = Math.min(1, MAX_DIMENSION / longest);
  const width = Math.max(1, Math.round(bitmap.width * scale));
  const height = Math.max(1, Math.round(bitmap.height * scale));

  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d', { willReadFrequently: true });
  if (!context) {
    bitmap.close();
    throw new Error('This browser did not provide a 2D canvas context.');
  }

  context.clearRect(0, 0, width, height);
  context.drawImage(bitmap, 0, 0, width, height);
  const originalWidth = bitmap.width;
  const originalHeight = bitmap.height;
  bitmap.close();

  // getImageData hands back straight (non-premultiplied) RGBA, which is what the
  // engine expects.
  const { data } = context.getImageData(0, 0, width, height);
  return { width, height, rgba: data.buffer, originalWidth, originalHeight };
}

function formatBytes(count: number) {
  if (count < 1024) return `${count} B`;
  if (count < 1024 * 1024) return `${(count / 1024).toFixed(1)} KB`;
  return `${(count / (1024 * 1024)).toFixed(1)} MB`;
}

/** Matches Tailwind's `lg` breakpoint, where the options become a real column. */
const DESKTOP_QUERY = '(min-width: 1024px)';

function useIsDesktop() {
  const [isDesktop, setIsDesktop] = useState(
    () =>
      typeof window !== 'undefined' && window.matchMedia(DESKTOP_QUERY).matches,
  );

  useEffect(() => {
    const query = window.matchMedia(DESKTOP_QUERY);
    const update = () => setIsDesktop(query.matches);
    update();
    query.addEventListener('change', update);
    return () => query.removeEventListener('change', update);
  }, []);

  return isDesktop;
}

export default function App() {
  const [options, setOptions] = useState<Options>(defaults);
  const [file, setFile] = useState<File | null>(null);
  const [sourceUrl, setSourceUrl] = useState<string | null>(null);
  const [decoded, setDecoded] = useState<Decoded | null>(null);
  const [report, setReport] = useState<VectorizeReport | null>(null);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [sheetOpen, setSheetOpen] = useState(false);

  const worker = useRef<Worker | null>(null);
  const nextId = useRef(0);
  const latestId = useRef(0);
  const sheet = useRef<HTMLElement | null>(null);
  const resultPane = useRef<HTMLDivElement | null>(null);

  const isDesktop = useIsDesktop();
  // Read inside an effect that must not re-run per report; a ref avoids adding
  // `report` to the dependency list.
  const hasReport = useRef(false);
  hasReport.current = report !== null;

  useEffect(() => {
    const instance = new Worker(
      new URL('./vectorizer.worker.ts', import.meta.url),
      {
        type: 'module',
      },
    );

    instance.onmessage = (event: MessageEvent<VectorizeResponse>) => {
      const message = event.data;
      // Options can change while a run is in flight; only the newest counts.
      if (message.id !== latestId.current) return;

      if (message.ok) {
        setReport(message.report);
        setElapsedMs(message.elapsedMs);
        setError(null);
      } else {
        setReport(null);
        setError(message.error);
      }
      setBusy(false);
    };

    instance.onerror = (event) => {
      setError(event.message || 'The vectorizer worker failed to start.');
      setBusy(false);
    };

    worker.current = instance;
    return () => instance.terminate();
  }, []);

  const accept = useCallback(async (next: File) => {
    setError(null);
    setReport(null);
    setElapsedMs(null);
    setFile(next);
    setSourceUrl((previous) => {
      if (previous) URL.revokeObjectURL(previous);
      return URL.createObjectURL(next);
    });

    try {
      setDecoded(await decode(next));
    } catch (cause) {
      setDecoded(null);
      setError(
        cause instanceof Error ? cause.message : 'Could not read that image.',
      );
    }
  }, []);

  // Re-run whenever the image or the options change, debounced so dragging a
  // slider does not queue a run per frame.
  useEffect(() => {
    if (!decoded || !worker.current) return;

    const timer = window.setTimeout(() => {
      const id = ++nextId.current;
      latestId.current = id;
      setBusy(true);

      // The buffer is copied because the same pixels are reused for every
      // subsequent run; transferring would empty it after the first.
      const request: VectorizeRequest = {
        id,
        width: decoded.width,
        height: decoded.height,
        rgba: decoded.rgba.slice(0),
        optionsJson: JSON.stringify(options),
      };
      worker.current?.postMessage(request, [request.rgba]);
    }, 120);

    return () => window.clearTimeout(timer);
  }, [decoded, options]);

  function onFileInput(event: ChangeEvent<HTMLInputElement>) {
    const next = event.target.files?.[0];
    if (next) void accept(next);
  }

  function onDrop(event: DragEvent<HTMLLabelElement>) {
    event.preventDefault();
    setDragging(false);
    const next = event.dataTransfer.files?.[0];
    if (next) void accept(next);
  }

  function download() {
    if (!report || !file) return;
    const blob = new Blob([report.svg], { type: 'image/svg+xml' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `${file.name.replace(/\.[^.]+$/, '')}.svg`;
    anchor.click();
    URL.revokeObjectURL(url);
  }

  // The panel is one element that becomes a bottom sheet below `lg`. While it is
  // parked off-screen it must leave the tab order, or focus disappears into a
  // panel nobody can see.
  useEffect(() => {
    const node = sheet.current;
    if (!node) return;
    (node as HTMLElement & { inert: boolean }).inert = !isDesktop && !sheetOpen;
  }, [isDesktop, sheetOpen]);

  useEffect(() => {
    if (isDesktop) setSheetOpen(false);
  }, [isDesktop]);

  useEffect(() => {
    if (isDesktop || !sheetOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setSheetOpen(false);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isDesktop, sheetOpen]);

  // Opening the sheet pulls the *result* pane to the top of the screen. Aligning
  // the whole results section instead would put the Source pane on screen and
  // leave the SVG behind the sheet, since the two panes stack below `sm` - which
  // defeats the point of being able to reach the sliders at all.
  useEffect(() => {
    if (isDesktop || !sheetOpen || !hasReport.current) return;
    resultPane.current?.scrollIntoView({ block: 'start', behavior: 'smooth' });
  }, [isDesktop, sheetOpen]);

  const downscaled = decoded && decoded.width !== decoded.originalWidth;

  // While the sheet is up it occupies 60% of a phone screen, so the preview is
  // capped to what is left. Without this the bottom third of the result sits
  // behind the panel you are adjusting it with.
  const previewMax = !isDesktop && sheetOpen ? '190px' : '420px';

  return (
    <div className="min-h-screen bg-slate-950 text-slate-100">
      <div className="mx-auto max-w-6xl px-6 py-10">
        <header className="mb-8">
          <h1 className="text-3xl font-semibold tracking-tight">
            Open Vectorizer
          </h1>
          <p className="mt-2 max-w-2xl text-sm leading-relaxed text-slate-400">
            Raster to clean SVG, entirely in your browser. Nothing is uploaded —
            the engine is compiled to WebAssembly and runs on this page.
            Circles, ellipses and rectangles come back as real SVG elements.
          </p>
        </header>

        <div className="lg:grid lg:gap-6 lg:grid-cols-[320px_1fr]">
          {/* Left column on desktop. On mobile only the drop zone is in flow
              here, because the panel below it becomes a fixed bottom sheet. */}
          <div>
            <label
              onDragOver={(event) => {
                event.preventDefault();
                setDragging(true);
              }}
              onDragLeave={() => setDragging(false)}
              onDrop={onDrop}
              className={clsx(
                'mb-6 flex cursor-pointer flex-col items-center justify-center rounded-lg border-2 border-dashed p-6 text-center transition',
                dragging
                  ? 'border-sky-400 bg-sky-400/10'
                  : 'border-slate-700 hover:border-slate-500',
              )}
            >
              <input
                type="file"
                accept="image/*"
                className="hidden"
                onChange={onFileInput}
              />
              <span className="text-sm font-medium">
                {file ? file.name : 'Drop an image, or click to choose'}
              </span>
              <span className="mt-1 text-xs text-slate-500">
                PNG, JPEG, WebP, GIF — anything your browser can decode
              </span>
            </label>

            <aside
              ref={sheet}
              id="options-panel"
              aria-label="Vectorizer options"
              className={clsx(
                // Below `lg` this is a bottom sheet. Deliberately not modal and
                // with no backdrop: the result has to stay visible and undimmed
                // while a slider moves, which is the entire point of it.
                'fixed inset-x-0 bottom-0 z-40 max-h-[60vh] overflow-y-auto rounded-t-2xl border-t border-slate-800 bg-slate-900 px-4 pb-[calc(1.5rem+env(safe-area-inset-bottom))] shadow-2xl shadow-slate-950 transition-transform duration-300 ease-out motion-reduce:transition-none',
                sheetOpen ? 'translate-y-0' : 'translate-y-full',
                // At `lg` it goes back to being an ordinary column in the flow.
                'lg:static lg:z-auto lg:max-h-none lg:translate-y-0 lg:overflow-visible lg:rounded-none lg:border-0 lg:bg-transparent lg:px-0 lg:pb-0 lg:shadow-none',
              )}
            >
              {/* Sticky so the close control stays reachable however far the
                  sheet is scrolled. */}
              <div className="sticky top-0 z-10 -mx-4 mb-4 bg-slate-900/95 px-4 pb-2 pt-3 backdrop-blur lg:hidden">
                <div
                  className="mx-auto mb-3 h-1 w-10 rounded-full bg-slate-700"
                  aria-hidden="true"
                />
                <div className="flex items-center justify-between">
                  <h2 className="text-sm font-semibold">Options</h2>
                  <button
                    type="button"
                    onClick={() => setSheetOpen(false)}
                    aria-label="Close options"
                    className="-mr-1 rounded-md p-2 text-slate-400 transition hover:bg-slate-800 hover:text-slate-100"
                  >
                    <svg
                      viewBox="0 0 20 20"
                      className="h-5 w-5"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.8"
                      strokeLinecap="round"
                      aria-hidden="true"
                    >
                      <path d="M5 5l10 10M15 5L5 15" />
                    </svg>
                  </button>
                </div>
              </div>

              <div className="space-y-6">
                <div>
                  <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
                    Presets
                  </h2>
                  <div className="grid grid-cols-2 gap-2">
                    {presets.map((preset) => (
                      <button
                        key={preset.label}
                        type="button"
                        title={preset.hint}
                        onClick={() => setOptions(preset.options)}
                        className={clsx(
                          'rounded-md border px-3 py-2 text-left text-sm transition',
                          options.mode === preset.options.mode
                            ? 'border-sky-500 bg-sky-500/10 text-sky-200'
                            : 'border-slate-700 hover:border-slate-500',
                        )}
                      >
                        {preset.label}
                      </button>
                    ))}
                  </div>
                </div>

                <div className="space-y-4">
                  <Slider
                    label="Colours"
                    value={options.colors}
                    min={2}
                    max={32}
                    step={1}
                    format={(value) => String(value)}
                    hint="Palette size ceiling. Near-identical colours are merged."
                    onChange={(colors) =>
                      setOptions((prev) => ({ ...prev, colors }))
                    }
                  />
                  <Slider
                    label="Detail"
                    value={options.detail}
                    min={0}
                    max={1}
                    step={0.05}
                    format={(value) => value.toFixed(2)}
                    hint="How much small structure to keep. Drives speck removal."
                    onChange={(detail) =>
                      setOptions((prev) => ({ ...prev, detail }))
                    }
                  />
                  <Slider
                    label="Smoothness"
                    value={options.smoothness}
                    min={0}
                    max={1}
                    step={0.05}
                    format={(value) => value.toFixed(2)}
                    hint="Evidence needed before a curve is broken by a corner."
                    onChange={(smoothness) =>
                      setOptions((prev) => ({ ...prev, smoothness }))
                    }
                  />
                  <Slider
                    label="Tolerance"
                    value={options.tolerance}
                    min={0.1}
                    max={10}
                    step={0.1}
                    format={(value) =>
                      `${value.toFixed(1)} (~${(value * 0.25).toFixed(2)}px)`
                    }
                    hint="Geometric error ceiling. A quarter of this is the budget in pixels."
                    onChange={(tolerance) =>
                      setOptions((prev) => ({ ...prev, tolerance }))
                    }
                  />
                </div>

                <button
                  type="button"
                  disabled={!report}
                  onClick={download}
                  className="w-full rounded-md bg-sky-500 px-4 py-2 text-sm font-medium text-slate-950 transition disabled:cursor-not-allowed disabled:bg-slate-800 disabled:text-slate-500"
                >
                  Download SVG
                </button>
              </div>
            </aside>
          </div>

          {/* Padded at the bottom on mobile so the floating button never covers
              the last row of statistics. */}
          <section className="space-y-4 pb-24 lg:pb-0">
            {error && (
              <div className="rounded-md border border-rose-800 bg-rose-950/50 px-4 py-3 text-sm text-rose-200">
                {error}
              </div>
            )}

            {downscaled && decoded && (
              <div className="rounded-md border border-amber-800/60 bg-amber-950/30 px-4 py-3 text-xs text-amber-200">
                Downscaled from {decoded.originalWidth}×{decoded.originalHeight}{' '}
                to {decoded.width}×{decoded.height}. Above {MAX_DIMENSION}px the
                coverage fields outgrow what WebAssembly can hold.
              </div>
            )}

            <div className="grid gap-4 sm:grid-cols-2">
              <Pane title="Source">
                {sourceUrl ? (
                  <img
                    src={sourceUrl}
                    alt="Source"
                    className="max-h-[420px] w-full object-contain"
                  />
                ) : (
                  <Empty>No image yet</Empty>
                )}
              </Pane>

              {/* Wrapped so the sheet can scroll the result — not the Source pane
                  above it — to the top of a phone screen. */}
              <div ref={resultPane}>
                <Pane title={busy ? 'Vectorizing…' : 'SVG'}>
                  {report ? (
                    <div
                      className="w-full [&>svg]:h-auto [&>svg]:max-h-[var(--preview-max)] [&>svg]:w-full"
                      style={
                        {
                          maxHeight: previewMax,
                          '--preview-max': previewMax,
                        } as React.CSSProperties
                      }
                      // The SVG is produced by our own engine from pixel data; it
                      // contains no scripts and no external references.
                      dangerouslySetInnerHTML={{ __html: report.svg }}
                    />
                  ) : (
                    <Empty>{busy ? 'Working…' : 'Nothing to show'}</Empty>
                  )}
                </Pane>
              </div>
            </div>

            {report && (
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                <Stat label="Shapes" value={String(report.shapes)} />
                <Stat label="Nodes" value={String(report.nodes)} />
                <Stat
                  label="Primitives"
                  value={`${report.circles}c ${report.ellipses}e ${report.rects}r`}
                />
                <Stat label="Accuracy" value={report.accuracy.toFixed(5)} />
                <Stat label="SVG size" value={formatBytes(report.svg.length)} />
                <Stat
                  label="Time"
                  value={
                    elapsedMs === null ? '—' : `${Math.round(elapsedMs)} ms`
                  }
                />
                <Stat
                  label="Input"
                  value={decoded ? `${decoded.width}×${decoded.height}` : '—'}
                />
                <Stat label="Mode" value={options.mode} />
              </div>
            )}

            {report && (
              <p className="text-xs leading-relaxed text-slate-500">
                Accuracy is 1 − mean absolute coverage error, measured by
                rendering the SVG back and comparing it to the source. Read it
                next to the node count: any vectorizer can buy accuracy by
                emitting more geometry.
              </p>
            )}
          </section>
        </div>
      </div>

      {/* Below `lg` the options are a sheet, so they need a way in. Hidden while
          the sheet is open, since the sheet carries its own close control and the
          button would otherwise sit on top of the sliders. */}
      <button
        type="button"
        onClick={() => setSheetOpen(true)}
        aria-expanded={sheetOpen}
        aria-controls="options-panel"
        className={clsx(
          'fixed right-4 z-50 flex items-center gap-2 rounded-full bg-sky-500 py-3 pl-4 pr-5 text-sm font-semibold text-slate-950 shadow-lg shadow-slate-950/60 transition lg:hidden',
          'bottom-[calc(1rem+env(safe-area-inset-bottom))]',
          sheetOpen && 'invisible opacity-0',
        )}
      >
        <svg
          viewBox="0 0 20 20"
          className="h-5 w-5"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          aria-hidden="true"
        >
          <path d="M3 6h9M15 6h2M3 14h3M9 14h8" />
          <circle cx="13.5" cy="6" r="2" />
          <circle cx="7.5" cy="14" r="2" />
        </svg>
        Options
      </button>
    </div>
  );
}

function Slider({
  label,
  value,
  min,
  max,
  step,
  format,
  hint,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format: (value: number) => string;
  hint: string;
  onChange: (value: number) => void;
}) {
  return (
    <div>
      <div className="flex items-baseline justify-between">
        <label className="text-sm font-medium">{label}</label>
        <span className="font-mono text-xs text-slate-400">
          {format(value)}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
        className="mt-1 w-full accent-sky-500"
      />
      <p className="mt-1 text-xs text-slate-500">{hint}</p>
    </div>
  );
}

function Pane({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-lg border border-slate-800 bg-slate-900/50">
      <div className="border-b border-slate-800 px-3 py-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
        {title}
      </div>
      <div className="flex min-h-[220px] items-center justify-center bg-[conic-gradient(#1e293b_90deg,transparent_90deg_180deg,#1e293b_180deg_270deg,transparent_270deg)] bg-[length:16px_16px] p-3">
        {children}
      </div>
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <span className="text-sm text-slate-600">{children}</span>;
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-slate-800 bg-slate-900/50 px-3 py-2">
      <div className="text-xs uppercase tracking-wide text-slate-500">
        {label}
      </div>
      <div className="mt-0.5 font-mono text-sm">{value}</div>
    </div>
  );
}
