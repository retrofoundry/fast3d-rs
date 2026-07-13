import init, { Renderer } from "../wasm/n64_toys.js";
import whiteUrl from "../white.png";
import type { Toy } from "../toys/types";
import { parseBin } from "./texture-bin";
import { emaFps } from "./fps";

export type Diagnostic = { line: number; msg: string };
export type Texture = { url: string; rgba: Uint8Array; w: number; h: number };
export type Settings = {
  autoRun: boolean;
  microcode: string;
  colorFormat: string;
  wireframe: boolean;
};

function sameDiags(a: Diagnostic[], b: Diagnostic[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i].line !== b[i].line || a[i].msg !== b[i].msg) return false;
  }
  return true;
}

function prefersReducedMotion(): boolean {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

async function decodeTexture(url: string): Promise<Texture> {
  // Raw RGBA `.bin` files (8-byte LE header: w u32, h u32; then w*h*4 RGBA bytes).
  // Bypasses the canvas/getImageData path so alpha-as-data (IA formats) is preserved.
  if (url.split("?")[0].endsWith(".bin")) {
    const buf = new Uint8Array(await (await fetch(url)).arrayBuffer());
    const { rgba, w, h } = parseBin(buf);
    return { url, rgba, w, h };
  }
  // PNG path: canvas round-trip via createImageBitmap + getImageData.
  // NOTE: this flattens straight alpha to opaque for textures whose alpha carries data
  // (IA formats) — not an issue for current toys (RGB/intensity channel only); revisit
  // if/when alpha-as-data textures render.
  const resp = await fetch(url);
  const blob = await resp.blob();
  const bitmap = await createImageBitmap(blob);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(bitmap, 0, 0);
  const imageData = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
  const tex = { url, rgba: new Uint8Array(imageData.data.buffer), w: bitmap.width, h: bitmap.height };
  bitmap.close();
  return tex;
}

export class Playground {
  source = $state("");
  diags = $state<Diagnostic[]>([]);
  status = $state("loading…");
  textures = $state<Texture[]>([]);
  title = $state("");
  description = $state("");
  forkOf: string | undefined;
  settings = $state<Settings>({
    autoRun: false,
    microcode: "F3DEX2",
    colorFormat: "RGBA16",
    wireframe: false,
  });

  // transport
  isAnimated = $state(false);
  playing = $state(false);
  errored = $state(false);
  time = $state(0); // seconds
  readonly scrubMax = 10; // seconds (unbounded model; default timeline window)
  fps = $state(0);

  #rafId: number | undefined;
  #startMs = 0;     // performance.now() when the current play segment began
  #baseTime = 0;    // accumulated play time before the current segment
  #fpsEma = 0;
  #lastFrameMs = 0;
  #lastFpsPushMs = 0;

  #renderer: Renderer | undefined;
  #initialized = false;
  #debounce: ReturnType<typeof setTimeout> | undefined;
  #lastObjectUrl: string | undefined;

  async init(canvas: HTMLCanvasElement): Promise<void> {
    if (this.#initialized) return; // guard: the editor view may mount more than once
    this.#initialized = true;
    await init({ module_or_path: new URL("../wasm/n64_toys_bg.wasm", import.meta.url) });
    this.#renderer = await Renderer.init(canvas);
    this.status = "ready";
    if (this.textures.length === 0) await this.loadTexture(whiteUrl);
    else this.run();
  }

  async loadTexture(src: string | File, { run = true } = {}): Promise<void> {
    let url: string;
    if (typeof src === "string") {
      url = src;
    } else {
      if (this.#lastObjectUrl) URL.revokeObjectURL(this.#lastObjectUrl);
      url = URL.createObjectURL(src);
      this.#lastObjectUrl = url;
    }
    const tex = await decodeTexture(url);
    this.textures = [tex];
    if (run) this.run();
  }

  /** Render a single frame at `t` seconds. Reads the typed RenderOut from wasm. */
  renderFrame(t: number): void {
    const tex = this.textures[0];
    if (!this.#renderer || !tex) return;
    const result = this.#renderer.render(this.source, t, tex.rgba, tex.w, tex.h) as {
      diags: Diagnostic[];
      is_time_variant: boolean;
      error: string | null;
    } | null;
    const diags = result?.diags ?? [];
    const errored = result?.error != null || diags.length > 0;
    // is_time_variant is now correct on every path (error path carries it via source_is_time_variant).
    // Set unconditionally; fall back to current value only if the result itself is null.
    this.isAnimated = result?.is_time_variant ?? this.isAnimated;
    // Push diags/status/errored only when they change (avoid 60fps reactivity + CodeMirror lint thrash).
    if (!sameDiags(this.diags, diags)) this.diags = diags;
    const status = result?.error
      ? `error: ${result.error}`
      : diags.length === 0
        ? "drew scene"
        : `${diags.length} diagnostic(s)`;
    if (this.status !== status) this.status = status;
    if (this.errored !== errored) this.errored = errored;
  }

  /** One-shot render at the current time (used by RUN button, autoRun debounce, init). Never auto-plays. */
  run(): void {
    this.renderFrame(this.time);
  }

  #loop = (): void => {
    if (!this.playing || !this.isAnimated) { this.pause(); return; }
    // §10: don't do work while the tab/canvas is hidden (rAF is throttled, but skip render too).
    if (typeof document !== "undefined" && document.hidden) {
      // Rebase the clock so no wall-time accrues while hidden (mirrors the errored-branch rebase).
      this.#baseTime = this.time;
      this.#startMs = performance.now();
      this.#lastFrameMs = 0;
      this.#rafId = requestAnimationFrame(this.#loop);
      return;
    }
    // While errored, FREEZE the clock (hold time) but keep re-rendering, so the next clean
    // assemble (e.g. an edit fixing a transient NaN) clears `errored` and resumes — §10/#14.
    if (!this.errored) {
      this.time = this.#baseTime + (performance.now() - this.#startMs) / 1000;
      if (this.time > this.scrubMax) {
        // loop the default timeline window
        this.#baseTime = 0;
        this.#startMs = performance.now();
        this.time = 0;
      }
    }
    const nowMs = performance.now();
    if (this.#lastFrameMs > 0) {
      this.#fpsEma = emaFps(this.#fpsEma, nowMs - this.#lastFrameMs);
      if (nowMs - this.#lastFpsPushMs >= 250) {
        this.fps = Math.round(this.#fpsEma);
        this.#lastFpsPushMs = nowMs;
      }
    }
    this.#lastFrameMs = nowMs;
    this.renderFrame(this.time); // sets this.errored; on error the wasm leaves the last good frame
    if (this.errored) {
      // rebase so playback continues from the frozen time (no jump) once it recovers
      this.#baseTime = this.time;
      this.#startMs = performance.now();
    }
    this.#rafId = requestAnimationFrame(this.#loop);
  };

  play(): void {
    if (this.playing || !this.isAnimated) return;
    this.playing = true;
    this.#baseTime = this.time;
    this.#startMs = performance.now();
    this.#rafId = requestAnimationFrame(this.#loop);
  }

  pause(): void {
    this.playing = false;
    if (this.#rafId != null) cancelAnimationFrame(this.#rafId);
    this.#rafId = undefined;
    this.fps = 0;
    this.#fpsEma = 0;
    this.#lastFrameMs = 0;
  }

  reset(): void {
    this.pause();
    this.time = 0;
    this.#baseTime = 0;
    this.renderFrame(0);
  }

  /** Seek to `t` seconds while paused (deterministic single-frame render). */
  seek(t: number): void {
    this.pause();
    this.time = t;
    this.renderFrame(t);
  }

  /** Cancel the loop + release the play head (call on editor exit). */
  teardown(): void {
    this.pause();
  }

  /** Load a toy into the editor as a transient Draft (the editor never mutates the persisted Toy). */
  async loadToy(toy: Toy): Promise<void> {
    this.pause(); // cancel any in-flight rAF loop from the previous toy
    this.source = toy.source;
    this.title = toy.title;
    this.description = toy.description;
    this.forkOf = toy.slug;
    this.time = 0;
    this.#baseTime = 0;
    await this.loadTexture(toy.texture ?? whiteUrl, { run: false });
    this.run(); // one-shot render at t=0; sets isAnimated from the typed result
    if (this.isAnimated && !prefersReducedMotion()) this.play();
  }

  scheduleAutoRun(): void {
    if (!this.settings.autoRun) return;
    clearTimeout(this.#debounce);
    this.#debounce = setTimeout(() => this.run(), 300);
  }
}
