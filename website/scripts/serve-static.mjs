#!/usr/bin/env node
import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import path from "node:path";

const root = path.resolve(process.cwd(), "out");
const port = Number(process.env.ISPER_WEBSITE_PORT || 3100);
const types = { ".css": "text/css; charset=utf-8", ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".json": "application/json; charset=utf-8", ".svg": "image/svg+xml", ".png": "image/png", ".woff2": "font/woff2", ".wasm": "application/wasm", ".xml": "application/xml; charset=utf-8" };

createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "127.0.0.1"}`);
  const decoded = decodeURIComponent(url.pathname);
  let file = path.resolve(root, `.${decoded}`);
  if (!file.startsWith(root)) { response.writeHead(403).end("Forbidden"); return; }
  try {
    const info = await stat(file);
    if (info.isDirectory()) file = path.join(file, "index.html");
    await stat(file);
    response.setHeader("Content-Type", types[path.extname(file)] ?? "application/octet-stream");
    response.setHeader("Cache-Control", "no-store");
    createReadStream(file).pipe(response);
  } catch {
    response.statusCode = 404;
    response.setHeader("Content-Type", "text/html; charset=utf-8");
    createReadStream(path.join(root, "404.html")).pipe(response);
  }
}).listen(port, "127.0.0.1", () => console.log(`ISPer website: http://127.0.0.1:${port}`));
