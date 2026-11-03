// Node side of the mock-host harness, as a Vite plugin:
//   GET  /__api/widgets                 the widget manifests found in the widget folders
//   GET  /__widget/<id>/<file>          a widget's files (what the real host serves per widget origin)
//   GET  /__api/plugins                 the plugin manifests found
//   POST /__api/plugin/<id>/call        { method, params } -> { result } | { error }, via a real plugin process
//   POST /__api/plugin/<id>/lifecycle   { action: "suspend" | "resume" }
// Plugin events go to the page over Vite's websocket ("mypaper:plugin-event"), and widget file
// changes as "mypaper:widget-changed" (the page reloads that widget).
//
// Folders: the repo's widgets/ and plugins/, plus MYPAPER_WIDGETS / MYPAPER_PLUGINS (a path list;
// each entry is a widget/plugin folder or a folder of them).

import { spawn, type ChildProcess } from "node:child_process";
import { createReadStream, existsSync, mkdirSync, readFileSync, readdirSync, statSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { tmpdir } from "node:os";
import { delimiter, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import type { Plugin } from "vite";

const repoRoot = fileURLToPath(new URL("../../..", import.meta.url));

const WIRE_CODES = ["PermissionDenied", "NotFound", "InvalidParams", "PluginUnavailable", "Internal"];
const IDLE_MS = 30_000;
const CALL_TIMEOUT_MS = 15_000;

// ------------------------------------------------------------------ folders

interface Found {
  id: string;
  dir: string;
  manifest: Record<string, unknown>;
}

function readJson(file: string): Record<string, unknown> | undefined {
  try {
    const value: unknown = JSON.parse(readFileSync(file, "utf8"));
    return typeof value === "object" && value !== null && !Array.isArray(value) ? { ...value } : undefined;
  } catch {
    return undefined;
  }
}

/** Finds `<manifestName>` files in each root (the root itself, or its child folders). Later roots win. */
function scan(roots: string[], manifestName: string): Found[] {
  const found = new Map<string, Found>();
  const add = (dir: string) => {
    const manifest = readJson(join(dir, manifestName));
    const id = manifest?.["id"];
    if (manifest && typeof id === "string") found.set(id, { id, dir, manifest });
  };
  for (const root of roots) {
    if (!existsSync(root)) continue;
    if (existsSync(join(root, manifestName))) add(root);
    else for (const e of readdirSync(root, { withFileTypes: true })) if (e.isDirectory()) add(join(root, e.name));
  }
  return [...found.values()];
}

function roots(repoFolder: string, env: string | undefined): string[] {
  return [join(repoRoot, repoFolder), ...(env ? env.split(delimiter).filter(Boolean).map((p) => resolve(p)) : [])];
}

const widgetRoots = () => roots("widgets", process.env["MYPAPER_WIDGETS"]);
const pluginRoots = () => roots("plugins", process.env["MYPAPER_PLUGINS"]);

// ------------------------------------------------------------------ plugin process

interface RpcError {
  code?: number;
  message?: string;
  data?: { code?: string };
}

class PluginCallError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

/** JSON-RPC error -> wire error (docs/widgets.md: `error.data.code` wins, then standard codes). */
function toWireError(err: RpcError): PluginCallError {
  const message = err.message ?? "plugin error";
  const data = err.data?.code;
  if (data && WIRE_CODES.includes(data)) return new PluginCallError(data, message);
  if (err.code === -32601) return new PluginCallError("NotFound", message);
  if (err.code === -32602) return new PluginCallError("InvalidParams", message);
  return new PluginCallError("Internal", message);
}

/** The plugin's executable: next to plugin.json, then the cargo target dirs, then PATH. */
function resolveCommand(dir: string, command: string): string {
  const candidates = [join(dir, command), join(repoRoot, "target", "release", command), join(repoRoot, "target", "debug", command)];
  return candidates.find((c) => existsSync(c)) ?? command;
}

class PluginProcess {
  private child: ChildProcess;
  private nextId = 1;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private idle: ReturnType<typeof setTimeout> | undefined;
  private exited = false;
  readonly ready: Promise<void>;

  constructor(
    readonly found: Found,
    private onEvent: (event: string, data: unknown) => void,
    private onExit: () => void,
  ) {
    const m = found.manifest;
    const command = typeof m["command"] === "string" ? m["command"] : "";
    const args = Array.isArray(m["args"]) ? m["args"].filter((a): a is string => typeof a === "string") : [];
    this.child = spawn(resolveCommand(found.dir, command), args, {
      cwd: found.dir,
      windowsHide: true,
      stdio: ["pipe", "pipe", "pipe"],
    });
    const tag = `[plugin:${found.id}]`;
    this.child.on("error", (err) => this.fail(new Error(`${tag} cannot start "${command}": ${err.message}`)));
    this.child.on("exit", (code) => {
      this.fail(new Error(`${tag} exited (${code})`));
    });
    this.child.stderr?.on("data", (d: Buffer) => console.log(tag, String(d).trimEnd()));
    if (this.child.stdout) createInterface({ input: this.child.stdout }).on("line", (line) => this.onLine(line));

    const dataDir = join(tmpdir(), "mypaper-dev", found.id);
    mkdirSync(dataDir, { recursive: true });
    this.ready = this.request("initialize", { protocol: 1, plugin: found.id, dataDir }, 0).then(() => {});
    this.touch();
  }

  private fail(err: Error): void {
    if (this.exited) return;
    this.exited = true;
    clearTimeout(this.idle);
    for (const p of this.pending.values()) p.reject(err);
    this.pending.clear();
    this.onExit();
  }

  private onLine(line: string): void {
    let msg: unknown;
    try {
      msg = JSON.parse(line);
    } catch {
      console.log(`[plugin:${this.found.id}] non-JSON on stdout: ${line}`);
      return;
    }
    if (typeof msg !== "object" || msg === null) return;
    if ("id" in msg && typeof msg.id === "number") {
      const p = this.pending.get(msg.id);
      if (!p) return;
      this.pending.delete(msg.id);
      if ("error" in msg && typeof msg.error === "object" && msg.error !== null) p.reject(toWireError(msg.error));
      else p.resolve("result" in msg ? msg.result : null);
    } else if ("method" in msg && msg.method === "event" && "params" in msg) {
      const params = msg.params;
      if (typeof params === "object" && params !== null && "event" in params && typeof params.event === "string") {
        this.onEvent(params.event, "data" in params ? params.data : null);
      }
    }
  }

  private request(method: string, params: unknown, id = this.nextId++): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new PluginCallError("PluginUnavailable", `plugin "${this.found.id}" did not answer "${method}"`));
      }, CALL_TIMEOUT_MS);
      this.pending.set(id, {
        resolve: (v) => (clearTimeout(timer), resolve(v)),
        reject: (e) => (clearTimeout(timer), reject(e)),
      });
      this.child.stdin?.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  }

  notify(method: string): void {
    this.child.stdin?.write(`${JSON.stringify({ jsonrpc: "2.0", method })}\n`);
  }

  async call(method: string, params: unknown): Promise<unknown> {
    try {
      await this.ready;
    } catch (err) {
      throw new PluginCallError("PluginUnavailable", err instanceof Error ? err.message : String(err));
    }
    this.touch();
    return this.request(method, params);
  }

  /** Stops the process after a quiet period, like the real host (30 s). */
  private touch(): void {
    clearTimeout(this.idle);
    this.idle = setTimeout(() => void this.stop(), IDLE_MS);
  }

  async stop(): Promise<void> {
    if (this.exited) return;
    clearTimeout(this.idle);
    const kill = setTimeout(() => this.child.kill(), 2000);
    await this.request("shutdown", undefined).catch(() => {});
    clearTimeout(kill);
    this.child.kill();
  }

  kill(): void {
    this.child.kill();
  }
}

// ------------------------------------------------------------------ http helpers

const MIME: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".ttf": "font/ttf",
};

function sendJson(res: ServerResponse, status: number, body: unknown): void {
  res.statusCode = status;
  res.setHeader("content-type", "application/json; charset=utf-8");
  res.setHeader("cache-control", "no-store");
  res.end(JSON.stringify(body));
}

async function readBody(req: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) chunks.push(Buffer.from(chunk as Uint8Array));
  try {
    const value: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
    return typeof value === "object" && value !== null ? { ...value } : {};
  } catch {
    return {};
  }
}

// ------------------------------------------------------------------ the plugin

export function devHostPlugin(): Plugin {
  return {
    name: "mypaper-dev-host",
    configureServer(server) {
      const processes = new Map<string, PluginProcess>();
      const killAll = () => processes.forEach((p) => p.kill());
      server.httpServer?.on("close", killAll);
      process.on("exit", killAll);

      server.watcher.add(widgetRoots().filter((r) => existsSync(r)));
      const onFile = (file: string) => {
        const hit = scan(widgetRoots(), "widget.json").find((w) => file.startsWith(w.dir + sep));
        if (hit) server.ws.send({ type: "custom", event: "mypaper:widget-changed", data: { id: hit.id } });
      };
      server.watcher.on("change", onFile).on("add", onFile);

      function pluginFor(id: string): PluginProcess | undefined {
        const existing = processes.get(id);
        if (existing) return existing;
        const found = scan(pluginRoots(), "plugin.json").find((p) => p.id === id);
        if (!found) return undefined;
        const proc = new PluginProcess(
          found,
          (event, data) => server.ws.send({ type: "custom", event: "mypaper:plugin-event", data: { plugin: id, event, data } }),
          () => processes.delete(id),
        );
        processes.set(id, proc);
        return proc;
      }

      server.middlewares.use(async (req, res, next) => {
        const url = new URL(req.url ?? "/", "http://localhost");
        const path = decodeURIComponent(url.pathname);

        if (path === "/__api/widgets") {
          return sendJson(res, 200, scan(widgetRoots(), "widget.json").map((w) => ({ id: w.id, manifest: w.manifest })));
        }

        if (path === "/__api/plugins") {
          return sendJson(
            res,
            200,
            scan(pluginRoots(), "plugin.json").map((p) => {
              const command = typeof p.manifest["command"] === "string" ? p.manifest["command"] : "";
              const resolved = resolveCommand(p.dir, command);
              return { id: p.id, manifest: p.manifest, runnable: existsSync(resolved) };
            }),
          );
        }

        const widgetMatch = /^\/__widget\/([a-z0-9-]+)\/(.*)$/.exec(path);
        if (widgetMatch) {
          const [, id = "", rest = ""] = widgetMatch;
          const dir = scan(widgetRoots(), "widget.json").find((w) => w.id === id)?.dir;
          if (!dir) return sendJson(res, 404, { error: `no widget "${id}"` });
          const file = resolve(dir, rest || "index.html");
          if (!file.startsWith(resolve(dir) + sep) || !existsSync(file) || !statSync(file).isFile()) {
            res.statusCode = 404;
            return res.end("not found");
          }
          res.setHeader("content-type", MIME[extname(file).toLowerCase()] ?? "application/octet-stream");
          res.setHeader("cache-control", "no-store");
          return void createReadStream(file).pipe(res);
        }

        const pluginMatch = /^\/__api\/plugin\/([a-z0-9-]+)\/(call|lifecycle)$/.exec(path);
        if (pluginMatch && req.method === "POST") {
          const [, id = "", action = ""] = pluginMatch;
          const body = await readBody(req);
          const proc = pluginFor(id);
          if (!proc) return sendJson(res, 200, { error: { code: "PluginUnavailable", message: `no plugin "${id}" found` } });
          if (action === "lifecycle") {
            if (body["action"] === "suspend" || body["action"] === "resume") proc.notify(body["action"]);
            return sendJson(res, 200, { result: null });
          }
          const method = body["method"];
          const methods = proc.found.manifest["methods"];
          if (typeof method !== "string" || !Array.isArray(methods) || !methods.includes(method)) {
            return sendJson(res, 200, { error: { code: "NotFound", message: `plugin "${id}" has no method "${String(method)}"` } });
          }
          try {
            return sendJson(res, 200, { result: await proc.call(method, body["params"]) });
          } catch (err) {
            const e = err instanceof PluginCallError ? err : new PluginCallError("Internal", String(err));
            return sendJson(res, 200, { error: { code: e.code, message: e.message } });
          }
        }

        next();
      });
    },
  };
}
