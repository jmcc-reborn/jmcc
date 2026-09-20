"use strict";

const vscode = require("vscode");
const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");
const https = require("https");
const http = require("http");

/** @type {{ stop: () => Promise<void> } | undefined} */
let client;

const TOKEN_TYPES = [
  "type",
  "class",
  "enum",
  "interface",
  "function",
  "method",
  "parameter",
  "variable",
  "property",
  "keyword",
  "string",
  "number",
  "operator",
  "enumMember",
  "event",
];

const TOKEN_MODIFIERS = ["declaration", "readonly", "static", "defaultLibrary"];
const legend = new vscode.SemanticTokensLegend(TOKEN_TYPES, TOKEN_MODIFIERS);

/**
 * Built-in zero-dependency LSP client over stdio.
 */
class BuiltinLspClient {
  constructor(serverPath, outputChannel, stdPath) {
    this.serverPath = serverPath;
    this.output = outputChannel;
    this.stdPath = stdPath || null;
    this.process = null;
    this.nextId = 1;
    this.pending = new Map();
    this.buffer = Buffer.alloc(0);
    this.diagnostics = vscode.languages.createDiagnosticCollection("justcode");
    this.disposables = [];
  }

  start() {
    return new Promise((resolve, reject) => {
      try {
        const args = ["lsp"];
        if (this.stdPath) {
          args.push("--std-path", this.stdPath);
        }
        const env = { ...process.env };
        if (this.stdPath) {
          env.JMCC_STD_PATH = this.stdPath;
        }
        this.process = spawn(this.serverPath, args, {
          stdio: ["pipe", "pipe", "pipe"],
          env,
        });
      } catch (err) {
        return reject(err);
      }

      this.process.on("error", (err) => {
        this.output.appendLine(`[jmc-analyzer] Process error: ${err.message}`);
        reject(err);
      });

      this.process.stderr.on("data", (chunk) => {
        this.output.append(`[jmc-analyzer stderr] ${chunk.toString("utf8")}`);
      });

      this.process.stdout.on("data", (chunk) => {
        this.onData(chunk);
      });

      this.process.on("exit", (code, signal) => {
        this.output.appendLine(
          `[jmc-analyzer] Server process exited with code ${code}, signal ${signal}`
        );
      });

      // Send initialize
      const rootUri =
        vscode.workspace.workspaceFolders &&
        vscode.workspace.workspaceFolders.length > 0
          ? vscode.workspace.workspaceFolders[0].uri.toString()
          : null;

      this.sendRequest("initialize", {
        processId: process.pid,
        rootUri: rootUri,
        initializationOptions: this.stdPath ? { stdPath: this.stdPath } : {},
        capabilities: {
          textDocument: {
            synchronization: { didSave: true },
            completion: {},
            hover: {},
            definition: {},
            rename: { prepareSupport: true },
            semanticTokens: {
              requests: { full: true },
              tokenTypes: TOKEN_TYPES,
              tokenModifiers: TOKEN_MODIFIERS,
            },
          },
        },
      })
        .then((res) => {
          this.sendNotification("initialized", {});
          this.output.appendLine("[jmc-analyzer] LSP server initialized successfully.");
          this.registerProviders();
          this.registerDocumentEvents();
          resolve();
        })
        .catch(reject);
    });
  }

  send(msg) {
    if (!this.process || !this.process.stdin.writable) return;
    const body = JSON.stringify(msg);
    const byteLen = Buffer.byteLength(body, "utf8");
    const header = `Content-Length: ${byteLen}\r\n\r\n`;
    this.process.stdin.write(header + body, "utf8");
  }

  sendRequest(method, params) {
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      this.pending.set(id, { resolve, reject });
      this.send({ jsonrpc: "2.0", id, method, params });
    });
  }

  sendNotification(method, params) {
    this.send({ jsonrpc: "2.0", method, params });
  }

  onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    while (true) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) break;

      const headerText = this.buffer.slice(0, headerEnd).toString("utf8");
      const match = /Content-Length:\s*(\d+)/i.exec(headerText);
      if (!match) {
        this.buffer = this.buffer.slice(headerEnd + 4);
        continue;
      }

      const contentLength = parseInt(match[1], 10);
      const totalMsgLength = headerEnd + 4 + contentLength;
      if (this.buffer.length < totalMsgLength) break;

      const bodyText = this.buffer
        .slice(headerEnd + 4, totalMsgLength)
        .toString("utf8");
      this.buffer = this.buffer.slice(totalMsgLength);

      try {
        const msg = JSON.parse(bodyText);
        this.handleMessage(msg);
      } catch (e) {
        this.output.appendLine(`[jmc-analyzer] Failed to parse JSON-RPC: ${e.message}`);
      }
    }
  }

  handleMessage(msg) {
    if (msg.id !== undefined && this.pending.has(msg.id)) {
      const { resolve, reject } = this.pending.get(msg.id);
      this.pending.delete(msg.id);
      if (msg.error) {
        reject(new Error(msg.error.message || "LSP request failed"));
      } else {
        resolve(msg.result);
      }
      return;
    }

    if (msg.method === "textDocument/publishDiagnostics") {
      this.handleDiagnostics(msg.params);
    }
  }

  handleDiagnostics(params) {
    const uri = vscode.Uri.parse(params.uri);
    const diags = (params.diagnostics || []).map((d) => {
      const range = new vscode.Range(
        d.range.start.line,
        d.range.start.character,
        d.range.end.line,
        d.range.end.character
      );
      let severity = vscode.DiagnosticSeverity.Error;
      if (d.severity === 2) severity = vscode.DiagnosticSeverity.Warning;
      else if (d.severity === 3) severity = vscode.DiagnosticSeverity.Information;
      else if (d.severity === 4) severity = vscode.DiagnosticSeverity.Hint;

      const diag = new vscode.Diagnostic(range, d.message, severity);
      diag.source = d.source || "jmc-analyzer";
      if (d.code) diag.code = d.code;
      return diag;
    });
    this.diagnostics.set(uri, diags);
  }

  registerDocumentEvents() {
    // Open open documents
    for (const doc of vscode.workspace.textDocuments) {
      if (doc.languageId === "jc") {
        this.sendNotification("textDocument/didOpen", {
          textDocument: {
            uri: doc.uri.toString(),
            languageId: "jc",
            version: doc.version,
            text: doc.getText(),
          },
        });
      }
    }

    this.disposables.push(
      vscode.workspace.onDidOpenTextDocument((doc) => {
        if (doc.languageId === "jc") {
          this.sendNotification("textDocument/didOpen", {
            textDocument: {
              uri: doc.uri.toString(),
              languageId: "jc",
              version: doc.version,
              text: doc.getText(),
            },
          });
        }
      }),
      vscode.workspace.onDidChangeTextDocument((e) => {
        if (e.document.languageId === "jc") {
          this.sendNotification("textDocument/didChange", {
            textDocument: {
              uri: e.document.uri.toString(),
              version: e.document.version,
            },
            contentChanges: [{ text: e.document.getText() }],
          });
        }
      }),
      vscode.workspace.onDidSaveTextDocument((doc) => {
        if (doc.languageId === "jc") {
          this.sendNotification("textDocument/didSave", {
            textDocument: { uri: doc.uri.toString() },
          });
        }
      }),
      vscode.workspace.onDidCloseTextDocument((doc) => {
        if (doc.languageId === "jc") {
          this.sendNotification("textDocument/didClose", {
            textDocument: { uri: doc.uri.toString() },
          });
          this.diagnostics.delete(doc.uri);
        }
      })
    );
  }

  registerProviders() {
    // 1. Completion Provider
    this.disposables.push(
      vscode.languages.registerCompletionItemProvider(
        "jc",
        {
          provideCompletionItems: async (document, position) => {
            try {
              const res = await this.sendRequest("textDocument/completion", {
                textDocument: { uri: document.uri.toString() },
                position: { line: position.line, character: position.character },
              });
              if (!res) return [];
              const items = Array.isArray(res) ? res : res.items || [];
              return items.map((item) => {
                const ci = new vscode.CompletionItem(item.label, item.kind - 1);
                if (item.detail) ci.detail = item.detail;
                if (item.documentation) {
                  const docVal =
                    typeof item.documentation === "string"
                      ? item.documentation
                      : item.documentation.value;
                  ci.documentation = new vscode.MarkdownString(docVal);
                }
                if (item.insertText) {
                  ci.insertText =
                    item.insertTextFormat === 2
                      ? new vscode.SnippetString(item.insertText)
                      : item.insertText;
                }
                return ci;
              });
            } catch {
              return [];
            }
          },
        },
        ":",
        "."
      )
    );

    // 2. Hover Provider
    this.disposables.push(
      vscode.languages.registerHoverProvider("jc", {
        provideHover: async (document, position) => {
          try {
            const res = await this.sendRequest("textDocument/hover", {
              textDocument: { uri: document.uri.toString() },
              position: { line: position.line, character: position.character },
            });
            if (!res || !res.contents) return null;
            const contentVal =
              typeof res.contents === "string"
                ? res.contents
                : res.contents.value || "";
            const range = res.range
              ? new vscode.Range(
                  res.range.start.line,
                  res.range.start.character,
                  res.range.end.line,
                  res.range.end.character
                )
              : undefined;
            return new vscode.Hover(new vscode.MarkdownString(contentVal), range);
          } catch {
            return null;
          }
        },
      })
    );

    // 3. Definition Provider
    this.disposables.push(
      vscode.languages.registerDefinitionProvider("jc", {
        provideDefinition: async (document, position) => {
          try {
            const res = await this.sendRequest("textDocument/definition", {
              textDocument: { uri: document.uri.toString() },
              position: { line: position.line, character: position.character },
            });
            if (!res) return null;
            if (Array.isArray(res)) {
              return res.map(
                (loc) =>
                  new vscode.Location(
                    vscode.Uri.parse(loc.uri),
                    new vscode.Range(
                      loc.range.start.line,
                      loc.range.start.character,
                      loc.range.end.line,
                      loc.range.end.character
                    )
                  )
              );
            }
            return new vscode.Location(
              vscode.Uri.parse(res.uri),
              new vscode.Range(
                res.range.start.line,
                res.range.start.character,
                res.range.end.line,
                res.range.end.character
              )
            );
          } catch {
            return null;
          }
        },
      })
    );

    // 4. Rename Provider
    this.disposables.push(
      vscode.languages.registerRenameProvider("jc", {
        prepareRename: async (document, position) => {
          try {
            const res = await this.sendRequest("textDocument/prepareRename", {
              textDocument: { uri: document.uri.toString() },
              position: { line: position.line, character: position.character },
            });
            if (!res) return null;
            const r = res.range || res;
            return new vscode.Range(
              r.start.line,
              r.start.character,
              r.end.line,
              r.end.character
            );
          } catch {
            return null;
          }
        },
        provideRenameEdits: async (document, position, newName) => {
          try {
            const res = await this.sendRequest("textDocument/rename", {
              textDocument: { uri: document.uri.toString() },
              position: { line: position.line, character: position.character },
              newName,
            });
            if (!res || !res.changes) return null;
            const edit = new vscode.WorkspaceEdit();
            for (const [uriStr, textEdits] of Object.entries(res.changes)) {
              const fileUri = vscode.Uri.parse(uriStr);
              for (const te of textEdits) {
                edit.replace(
                  fileUri,
                  new vscode.Range(
                    te.range.start.line,
                    te.range.start.character,
                    te.range.end.line,
                    te.range.end.character
                  ),
                  te.newText
                );
              }
            }
            return edit;
          } catch {
            return null;
          }
        },
      })
    );

    // 5. Semantic Tokens Provider
    this.disposables.push(
      vscode.languages.registerDocumentSemanticTokensProvider(
        "jc",
        {
          provideDocumentSemanticTokens: async (document) => {
            try {
              const res = await this.sendRequest("textDocument/semanticTokens/full", {
                textDocument: { uri: document.uri.toString() },
              });
              if (!res || !res.data) return null;
              const builder = new vscode.SemanticTokensBuilder(legend);
              const data = res.data;
              let line = 0;
              let startChar = 0;
              for (let i = 0; i < data.length; i += 5) {
                const deltaLine = data[i];
                const deltaStart = data[i + 1];
                const length = data[i + 2];
                const tokenType = data[i + 3];
                const tokenModifiers = data[i + 4];

                line += deltaLine;
                if (deltaLine > 0) {
                  startChar = deltaStart;
                } else {
                  startChar += deltaStart;
                }

                builder.push(line, startChar, length, tokenType, tokenModifiers);
              }
              return builder.build();
            } catch {
              return null;
            }
          },
        },
        legend
      )
    );

    // 6. Document Symbol Provider (Outline / Breadcrumbs)
    this.disposables.push(
      vscode.languages.registerDocumentSymbolProvider("jc", {
        provideDocumentSymbols: async (document) => {
          try {
            const res = await this.sendRequest("textDocument/documentSymbol", {
              textDocument: { uri: document.uri.toString() },
            });
            if (!res) return [];
            const toSymbol = (s) => {
              const range = new vscode.Range(
                s.range.start.line,
                s.range.start.character,
                s.range.end.line,
                s.range.end.character
              );
              const selRange = new vscode.Range(
                s.selectionRange.start.line,
                s.selectionRange.start.character,
                s.selectionRange.end.line,
                s.selectionRange.end.character
              );
              const docSym = new vscode.DocumentSymbol(
                s.name,
                s.detail || "",
                s.kind - 1,
                range,
                selRange
              );
              if (s.children && s.children.length > 0) {
                docSym.children = s.children.map(toSymbol);
              }
              return docSym;
            };
            return res.map(toSymbol);
          } catch {
            return [];
          }
        },
      })
    );

    // 7. Document Formatting Provider
    this.disposables.push(
      vscode.languages.registerDocumentFormattingEditProvider("jc", {
        provideDocumentFormattingEdits: async (document, options) => {
          try {
            const res = await this.sendRequest("textDocument/formatting", {
              textDocument: { uri: document.uri.toString() },
              options: {
                tabSize: options.tabSize,
                insertSpaces: options.insertSpaces,
              },
            });
            if (!res) return [];
            return res.map(
              (edit) =>
                new vscode.TextEdit(
                  new vscode.Range(
                    edit.range.start.line,
                    edit.range.start.character,
                    edit.range.end.line,
                    edit.range.end.character
                  ),
                  edit.newText
                )
            );
          } catch {
            return [];
          }
        },
      })
    );

    // 8. Inlay Hints Provider
    this.disposables.push(
      vscode.languages.registerInlayHintsProvider("jc", {
        provideInlayHints: async (document, range) => {
          try {
            const res = await this.sendRequest("textDocument/inlayHint", {
              textDocument: { uri: document.uri.toString() },
              range: {
                start: { line: range.start.line, character: range.start.character },
                end: { line: range.end.line, character: range.end.character },
              },
            });
            if (!res) return [];
            return res.map((h) => {
              const label = typeof h.label === "string" ? h.label : h.label.value || "";
              const pos = new vscode.Position(h.position.line, h.position.character);
              const hint = new vscode.InlayHint(
                pos,
                label,
                h.kind === 1 ? vscode.InlayHintKind.Type : vscode.InlayHintKind.Parameter
              );
              hint.paddingLeft = h.paddingLeft;
              hint.paddingRight = h.paddingRight;
              return hint;
            });
          } catch {
            return [];
          }
        },
      })
    );

    // 9. Signature Help Provider
    this.disposables.push(
      vscode.languages.registerSignatureHelpProvider(
        "jc",
        {
          provideSignatureHelp: async (document, position) => {
            try {
              const res = await this.sendRequest("textDocument/signatureHelp", {
                textDocument: { uri: document.uri.toString() },
                position: { line: position.line, character: position.character },
              });
              if (!res || !res.signatures || res.signatures.length === 0) return null;
              const help = new vscode.SignatureHelp();
              help.activeSignature = res.activeSignature || 0;
              help.activeParameter = res.activeParameter || 0;
              help.signatures = res.signatures.map((sig) => {
                const info = new vscode.SignatureInformation(sig.label);
                if (sig.documentation) {
                  const docVal =
                    typeof sig.documentation === "string"
                      ? sig.documentation
                      : sig.documentation.value || "";
                  info.documentation = new vscode.MarkdownString(docVal);
                }
                if (sig.parameters) {
                  info.parameters = sig.parameters.map((p) => {
                    const pLabel = typeof p.label === "string" ? p.label : p.label;
                    return new vscode.ParameterInformation(pLabel);
                  });
                }
                return info;
              });
              return help;
            } catch {
              return null;
            }
          },
        },
        "(",
        ","
      )
    );

    // 10. Document Highlight Provider
    this.disposables.push(
      vscode.languages.registerDocumentHighlightProvider("jc", {
        provideDocumentHighlights: async (document, position) => {
          try {
            const res = await this.sendRequest("textDocument/documentHighlight", {
              textDocument: { uri: document.uri.toString() },
              position: { line: position.line, character: position.character },
            });
            if (!res) return [];
            return res.map(
              (h) =>
                new vscode.DocumentHighlight(
                  new vscode.Range(
                    h.range.start.line,
                    h.range.start.character,
                    h.range.end.line,
                    h.range.end.character
                  ),
                  h.kind === 2
                    ? vscode.DocumentHighlightKind.Write
                    : vscode.DocumentHighlightKind.Read
                )
            );
          } catch {
            return [];
          }
        },
      })
    );

    // 11. Reference Provider (Find References)
    this.disposables.push(
      vscode.languages.registerReferenceProvider("jc", {
        provideReferences: async (document, position, context) => {
          try {
            const res = await this.sendRequest("textDocument/references", {
              textDocument: { uri: document.uri.toString() },
              position: { line: position.line, character: position.character },
              context: { includeDeclaration: context.includeDeclaration },
            });
            if (!res) return [];
            return res.map(
              (loc) =>
                new vscode.Location(
                  vscode.Uri.parse(loc.uri),
                  new vscode.Range(
                    loc.range.start.line,
                    loc.range.start.character,
                    loc.range.end.line,
                    loc.range.end.character
                  )
                )
            );
          } catch {
            return [];
          }
        },
      })
    );

    // 12. Workspace Symbol Provider
    this.disposables.push(
      vscode.languages.registerWorkspaceSymbolProvider({
        provideWorkspaceSymbols: async (query) => {
          try {
            const res = await this.sendRequest("workspace/symbol", {
              query,
            });
            if (!res) return [];
            return res.map(
              (sym) =>
                new vscode.SymbolInformation(
                  sym.name,
                  sym.kind - 1,
                  sym.containerName || "",
                  new vscode.Location(
                    vscode.Uri.parse(sym.location.uri),
                    new vscode.Range(
                      sym.location.range.start.line,
                      sym.location.range.start.character,
                      sym.location.range.end.line,
                      sym.location.range.end.character
                    )
                  )
                )
            );
          } catch {
            return [];
          }
        },
      })
    );
  }

  async stop() {
    for (const d of this.disposables) {
      d.dispose();
    }
    this.disposables = [];
    this.diagnostics.clear();
    this.diagnostics.dispose();

    if (this.process) {
      try {
        await this.sendRequest("shutdown", {});
        this.sendNotification("exit", {});
      } catch {}
      this.process.kill();
      this.process = null;
    }
  }
}

function isExecutable(filePath) {
  try {
    const stat = fs.statSync(filePath);
    return stat.isFile();
  } catch {
    return false;
  }
}

function findInPath(name) {
  const envPath = process.env.PATH || "";
  const sep = process.platform === "win32" ? ";" : ":";
  const exts = process.platform === "win32" ? [".exe", ".cmd", ".bat", ""] : [""];
  for (const dir of envPath.split(sep)) {
    if (!dir) continue;
    for (const ext of exts) {
      const candidate = path.join(dir, name + ext);
      if (isExecutable(candidate)) {
        return candidate;
      }
    }
  }
  return null;
}

function getTargetAsset() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "win32") {
    return "jmc-analyzer-windows-x64.exe";
  } else if (platform === "linux") {
    if (arch === "arm64") return "jmc-analyzer-linux-arm64";
    return "jmc-analyzer-linux-x64";
  } else if (platform === "darwin") {
    if (arch === "arm64") return "jmc-analyzer-darwin-arm64";
    return "jmc-analyzer-darwin-x64";
  }
  return null;
}

function getGlobalStorageBinaryPath(context) {
  if (!context || !context.globalStorageUri) return null;
  const binaryName = process.platform === "win32" ? "jmc-analyzer.exe" : "jmc-analyzer";
  return path.join(context.globalStorageUri.fsPath, "bin", binaryName);
}

function downloadFile(url, destPath, onProgress, maxRedirects = 5) {
  return new Promise((resolve, reject) => {
    if (maxRedirects <= 0) {
      return reject(new Error("Слишком много перенаправлений при скачивании сервера"));
    }

    const client = url.startsWith("http://") ? http : https;
    const request = client.get(
      url,
      {
        headers: {
          "User-Agent": "vscode-jmc-analyzer",
          Accept: "application/octet-stream",
        },
      },
      (res) => {
        if (
          res.statusCode === 301 ||
          res.statusCode === 302 ||
          res.statusCode === 303 ||
          res.statusCode === 307 ||
          res.statusCode === 308
        ) {
          const redirectUrl = res.headers.location;
          if (!redirectUrl) {
            return reject(new Error(`Redirect status ${res.statusCode} without Location header`));
          }
          res.resume();
          return resolve(downloadFile(redirectUrl, destPath, onProgress, maxRedirects - 1));
        }

        if (res.statusCode !== 200) {
          res.resume();
          return reject(
            new Error(`HTTP ${res.statusCode} (${res.statusMessage || "Error"}) при загрузке ${url}`)
          );
        }

        const totalBytes = parseInt(res.headers["content-length"] || "0", 10);
        let downloadedBytes = 0;

        const tmpPath = destPath + ".tmp." + Date.now();
        const dir = path.dirname(destPath);
        try {
          fs.mkdirSync(dir, { recursive: true });
        } catch (e) {
          return reject(e);
        }

        const fileStream = fs.createWriteStream(tmpPath);

        res.on("data", (chunk) => {
          downloadedBytes += chunk.length;
          if (totalBytes > 0 && onProgress) {
            onProgress(downloadedBytes, totalBytes);
          }
        });

        res.pipe(fileStream);

        fileStream.on("finish", () => {
          fileStream.close(() => {
            try {
              if (fs.existsSync(destPath)) {
                fs.unlinkSync(destPath);
              }
              fs.renameSync(tmpPath, destPath);
              if (process.platform !== "win32") {
                fs.chmodSync(destPath, 0o755);
              }
              resolve();
            } catch (err) {
              reject(err);
            }
          });
        });

        fileStream.on("error", (err) => {
          try {
            if (fs.existsSync(tmpPath)) fs.unlinkSync(tmpPath);
          } catch {}
          reject(err);
        });
      }
    );

    request.on("error", (err) => {
      reject(err);
    });

    request.setTimeout(60000, () => {
      request.destroy();
      reject(new Error("Превышено время ожидания загрузки (таймаут 60 сек)"));
    });
  });
}

async function downloadServerBinary(context, output) {
  const assetName = getTargetAsset();
  if (!assetName) {
    throw new Error(`Неподдерживаемая платформа для автоскачивания: ${process.platform} ${process.arch}`);
  }

  const destPath = getGlobalStorageBinaryPath(context);
  if (!destPath) {
    throw new Error("Не удалось определить папку globalStorage расширения");
  }

  const downloadUrl = `https://github.com/jmcc-reborn/jmcc/releases/latest/download/${assetName}`;
  output.appendLine(`[jmc-analyzer] Downloading language server from ${downloadUrl}...`);

  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "JMC Analyzer",
      cancellable: false,
    },
    async (progress) => {
      progress.report({ message: `Скачивание ${assetName}...` });
      let lastPct = 0;
      await downloadFile(downloadUrl, destPath, (downloaded, total) => {
        const pct = Math.round((downloaded / total) * 100);
        if (pct > lastPct) {
          progress.report({
            message: `Скачивание ${assetName}: ${(downloaded / 1024 / 1024).toFixed(1)}MB / ${(total / 1024 / 1024).toFixed(1)}MB (${pct}%)`,
            increment: pct - lastPct,
          });
          lastPct = pct;
        }
      });
    }
  );

  output.appendLine(`[jmc-analyzer] Successfully downloaded server to ${destPath}`);
  return destPath;
}

function resolveServerBinary(configuredPath, output, context) {
  const binaryName = process.platform === "win32" ? "jmc-analyzer.exe" : "jmc-analyzer";

  // 1. If configured path is explicitly provided
  if (configuredPath && configuredPath !== "jmc-analyzer") {
    if (path.isAbsolute(configuredPath) && isExecutable(configuredPath)) {
      output.appendLine(`[jmc-analyzer] Using configured server executable: ${configuredPath}`);
      return configuredPath;
    }
    if (vscode.workspace.workspaceFolders) {
      for (const wf of vscode.workspace.workspaceFolders) {
        const candidate = path.resolve(wf.uri.fsPath, configuredPath);
        if (isExecutable(candidate)) {
          output.appendLine(`[jmc-analyzer] Found server executable in workspace: ${candidate}`);
          return candidate;
        }
      }
    }
  }

  // 2. Search in PATH
  const fromPath = findInPath("jmc-analyzer");
  if (fromPath) {
    output.appendLine(`[jmc-analyzer] Found server executable in PATH: ${fromPath}`);
    return fromPath;
  }

  // 3. Search in globalStorage (previously downloaded)
  const storagePath = getGlobalStorageBinaryPath(context);
  if (storagePath && isExecutable(storagePath)) {
    output.appendLine(`[jmc-analyzer] Found server executable in global storage: ${storagePath}`);
    return storagePath;
  }

  // 4. Search in ~/.cargo/bin
  const cargoBin = path.join(os.homedir(), ".cargo", "bin", binaryName);
  if (isExecutable(cargoBin)) {
    output.appendLine(`[jmc-analyzer] Found server executable in ~/.cargo/bin: ${cargoBin}`);
    return cargoBin;
  }

  // 5. Search in workspace target directories (release, then debug)
  if (vscode.workspace.workspaceFolders) {
    for (const wf of vscode.workspace.workspaceFolders) {
      const candidates = [
        path.join(wf.uri.fsPath, "target", "release", binaryName),
        path.join(wf.uri.fsPath, "target", "debug", binaryName),
        path.join(wf.uri.fsPath, "..", "target", "release", binaryName),
        path.join(wf.uri.fsPath, "..", "target", "debug", binaryName),
      ];
      for (const candidate of candidates) {
        if (isExecutable(candidate)) {
          output.appendLine(`[jmc-analyzer] Found built server executable: ${candidate}`);
          return candidate;
        }
      }
    }
  }

  // 6. Search relative to extension directory
  const bundled = [
    path.join(__dirname, "..", "server", binaryName),
    path.join(__dirname, "..", "..", "target", "release", binaryName),
    path.join(__dirname, "..", "..", "target", "debug", binaryName),
  ];
  for (const candidate of bundled) {
    if (isExecutable(candidate)) {
      output.appendLine(`[jmc-analyzer] Found server executable in fallback path: ${candidate}`);
      return candidate;
    }
  }

  return null;
}

async function startServer(context, output) {
  if (client) {
    try {
      await client.stop();
    } catch {}
    client = null;
  }

  const config = vscode.workspace.getConfiguration("jmc.analyzer");
  if (!config.get("enableLsp")) {
    output.appendLine(
      "LSP is disabled (jmc.analyzer.enableLsp). Syntax highlighting does not need the server."
    );
    return;
  }

  const configuredPath = config.get("serverPath") || "";
  let serverPath = resolveServerBinary(configuredPath, output, context);

  if (!serverPath) {
    const autoDownload = config.get("autoDownload") ?? true;
    if (autoDownload && getTargetAsset() && context && context.globalStorageUri) {
      try {
        serverPath = await downloadServerBinary(context, output);
        void vscode.window.showInformationMessage("Языковой сервер JMC Analyzer успешно загружен!");
      } catch (err) {
        output.appendLine(`[jmc-analyzer] Auto-download failed: ${err.message}`);
      }
    }
  }

  if (!serverPath) {
    const message =
      "jmc-analyzer language server binary not found. Build it with 'cargo build -p jmc-analyzer' or install to PATH.";
    output.appendLine(`[jmc-analyzer] ${message}`);
    void vscode.window
      .showErrorMessage(
        "Языковой сервер jmc-analyzer не найден.",
        "Скачать с GitHub",
        "Собрать (cargo build)",
        "Установить в PATH (cargo install)"
      )
      .then(async (choice) => {
        if (choice === "Скачать с GitHub") {
          try {
            const downloaded = await downloadServerBinary(context, output);
            void vscode.window.showInformationMessage("Языковой сервер JMC Analyzer успешно загружен!");
            await startServer(context, output);
          } catch (e) {
            void vscode.window.showErrorMessage(`Ошибка загрузки: ${e.message}`);
          }
        } else if (choice === "Собрать (cargo build)") {
          const terminal = vscode.window.createTerminal("jmc-analyzer build");
          terminal.show();
          terminal.sendText("cargo build -p jmc-analyzer");
        } else if (choice === "Установить в PATH (cargo install)") {
          const terminal = vscode.window.createTerminal("jmc-analyzer install");
          terminal.show();
          terminal.sendText("cargo install --path jmc-analyzer");
        }
      });
    return;
  }

  const configuredStdPath = config.get("stdPath") || "";
  const bundledStdPath = context ? path.join(context.extensionPath, "std") : "";
  const effectiveStdPath = configuredStdPath.trim() !== ""
    ? configuredStdPath.trim()
    : (bundledStdPath && fs.existsSync(bundledStdPath) ? bundledStdPath : "");

  if (effectiveStdPath) {
    output.appendLine(`[jmc-analyzer] Using standard library path: ${effectiveStdPath}`);
  }

  // Try vscode-languageclient if available
  let LanguageClient;
  try {
    ({ LanguageClient } = require("vscode-languageclient/node"));
  } catch {
    LanguageClient = null;
  }

  const lspArgs = ["lsp"];
  if (effectiveStdPath) {
    lspArgs.push("--std-path", effectiveStdPath);
  }
  const lspEnv = { ...process.env };
  if (effectiveStdPath) {
    lspEnv.JMCC_STD_PATH = effectiveStdPath;
  }

  if (LanguageClient) {
    output.appendLine(`Using vscode-languageclient with '${serverPath}'`);
    client = new LanguageClient(
      "jmc-analyzer",
      "JMC Analyzer",
      {
        command: serverPath,
        args: lspArgs,
        options: { env: lspEnv },
        initializationOptions: effectiveStdPath ? { stdPath: effectiveStdPath } : {},
      },
      {
        documentSelector: [{ language: "jc" }],
        outputChannel: output,
      }
    );
    await client.start();
  } else {
    output.appendLine(`Using built-in zero-dependency LSP client with '${serverPath}'`);
    const builtin = new BuiltinLspClient(serverPath, output, effectiveStdPath);
    client = builtin;
    try {
      await builtin.start();
    } catch (err) {
      const message = `jmc-analyzer LSP could not be started with '${serverPath}': ${err.message}`;
      output.appendLine(message);
      void vscode.window.showWarningMessage(message);
    }
  }
}

/**
 * @param {import("vscode").ExtensionContext} context
 */
async function activate(context) {
  const output = vscode.window.createOutputChannel("JMC Analyzer");
  context.subscriptions.push(output);

  context.subscriptions.push(
    vscode.commands.registerCommand("jmc.analyzer.restartServer", async () => {
      output.appendLine("[jmc-analyzer] Restarting language server...");
      await startServer(context, output);
    }),
    vscode.commands.registerCommand("jmc.analyzer.downloadServer", async () => {
      try {
        await downloadServerBinary(context, output);
        void vscode.window.showInformationMessage("Языковой сервер JMC Analyzer успешно обновлен!");
        await startServer(context, output);
      } catch (err) {
        void vscode.window.showErrorMessage(`Не удалось обновить сервер: ${err.message}`);
      }
    })
  );

  await startServer(context, output);
}

async function deactivate() {
  if (client) {
    await client.stop();
  }
}

module.exports = { activate, deactivate };

